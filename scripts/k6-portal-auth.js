/**
 * k6 Load Test: Portal Auth Stress
 *
 * Tests login endpoint performance (argon2id hashing latency) and verifies
 * brute-force protection triggers under concurrent login attempts.
 *
 * Usage:
 *   k6 run scripts/k6-portal-auth.js
 *
 * Environment variables:
 *   GATEWAY_URL  - Gateway URL (default: http://localhost:4040)
 *
 * Prerequisites:
 *   The script creates its own test accounts via signup during setup.
 */

import http from "k6/http";
import { check, sleep } from "k6";
import { Counter, Rate, Trend } from "k6/metrics";

const loginLatency = new Trend("login_latency_ms");
const bruteForceBlocked = new Counter("brute_force_blocked_429");
const loginErrors = new Rate("login_errors");

export const options = {
  scenarios: {
    // Scenario 1: Normal login/signup load
    auth_load: {
      executor: "constant-vus",
      vus: 10,
      duration: "30s",
      exec: "authLoad"
    },
    // Scenario 2: Brute-force simulation (single email, wrong passwords)
    brute_force: {
      executor: "constant-vus",
      vus: 3,
      duration: "15s",
      startTime: "5s", // Start after some normal logins
      exec: "bruteForce"
    }
  },
  thresholds: {
    login_latency_ms: ["p(95)<2000"], // argon2id is slow by design — 2s budget
    brute_force_blocked_429: ["count>0"], // Brute-force protection must trigger
    login_errors: ["rate<0.50"] // Allow failed logins (wrong pass scenario)
  }
};

const GATEWAY_URL = __ENV.GATEWAY_URL || "http://localhost:4040";
const ts = Date.now();

const jsonHeaders = {
  headers: { "Content-Type": "application/json" }
};

// Setup: create accounts for the auth load test
export function setup() {
  const accounts = [];

  // Create 10 accounts (one per VU in auth_load scenario)
  for (let i = 1; i <= 10; i++) {
    const email = `k6-auth-${ts}-${i}@test.local`;
    const password = "K6TestPass1";
    const orgName = `k6-auth-org-${ts}-${i}`;

    const res = http.post(
      `${GATEWAY_URL}/portal/v1/auth/signup`,
      JSON.stringify({
        org_name: orgName,
        name: `K6 Tester ${i}`,
        email: email,
        password: password
      }),
      jsonHeaders
    );

    if (res.status === 200) {
      accounts.push({ email, password });
    }
  }

  // Create brute-force target account
  const bfEmail = `k6-brute-target-${ts}@test.local`;
  const bfPassword = "BruteTarget1";
  http.post(
    `${GATEWAY_URL}/portal/v1/auth/signup`,
    JSON.stringify({
      org_name: `k6-brute-org-${ts}`,
      name: "Brute Target",
      email: bfEmail,
      password: bfPassword
    }),
    jsonHeaders
  );

  return { accounts, bruteEmail: bfEmail };
}

// Scenario 1: Normal auth load — login with valid credentials
export function authLoad(data) {
  if (!data.accounts || data.accounts.length === 0) return;

  const account = data.accounts[(__VU - 1) % data.accounts.length];

  const res = http.post(
    `${GATEWAY_URL}/portal/v1/auth/login`,
    JSON.stringify({
      email: account.email,
      password: account.password
    }),
    { ...jsonHeaders, tags: { name: "login_valid" } }
  );

  check(res, {
    "login success (200)": (r) => r.status === 200
  }) || loginErrors.add(1);

  loginLatency.add(res.timings.duration);
  sleep(1); // 1 RPS per VU to stay within rate limits
}

// Scenario 2: Brute-force simulation — wrong passwords on same email
export function bruteForce(data) {
  if (!data.bruteEmail) return;

  const res = http.post(
    `${GATEWAY_URL}/portal/v1/auth/login`,
    JSON.stringify({
      email: data.bruteEmail,
      password: `WrongPass${__ITER}`
    }),
    { ...jsonHeaders, tags: { name: "brute_force_attempt" } }
  );

  if (res.status === 429) {
    bruteForceBlocked.add(1);
    check(res, {
      "429 has Retry-After": (r) => r.headers["Retry-After"] !== undefined
    });
  } else {
    check(res, {
      "failed login is 401": (r) => r.status === 401
    });
  }

  sleep(0.2); // Fast enough to trigger brute-force protection
}

export function handleSummary(data) {
  const blocked = data.metrics.brute_force_blocked_429
    ? data.metrics.brute_force_blocked_429.values.count
    : 0;
  const loginP95 = data.metrics.login_latency_ms
    ? data.metrics.login_latency_ms.values["p(95)"].toFixed(0)
    : "N/A";

  console.log(`\n=== Portal Auth Stress Summary ===`);
  console.log(`Login p95 latency:    ${loginP95}ms`);
  console.log(`Brute-force blocked:  ${blocked} requests`);

  if (blocked === 0) {
    console.log(
      `\n⚠️  WARNING: No brute-force blocks — protection may not be working!`
    );
  } else {
    console.log(`\n✅ Brute-force protection triggered successfully.`);
  }

  return {};
}
