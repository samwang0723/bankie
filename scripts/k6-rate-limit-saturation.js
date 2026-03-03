/**
 * k6 Load Test: Rate Limit Saturation
 *
 * Verifies the Redis token bucket rate limiter works under heavy load.
 * Expects 429 responses when burst limit (100) is exceeded.
 *
 * Usage:
 *   k6 run -e API_KEY=bk_live_... scripts/k6-rate-limit-saturation.js
 *
 * Environment variables:
 *   GATEWAY_URL  - Gateway URL (default: http://localhost:4040)
 *   API_KEY      - Valid API key (will be rate-limited)
 *   ACCOUNT_ID   - Any valid account UUID (optional, uses dummy if not set)
 */

import http from "k6/http";
import { check } from "k6";
import { Counter, Rate, Trend } from "k6/metrics";

const rateLimited = new Counter("rate_limited_429");
const passed = new Counter("passed_200");
const rejectionLatency = new Trend("rejection_latency_ms");
const rateLimitRate = new Rate("rate_limit_rate");

export const options = {
  scenarios: {
    burst: {
      executor: "constant-vus",
      vus: 200,
      duration: "30s"
    }
  },
  thresholds: {
    // We EXPECT 429s — this test passes when rate limiting works
    rate_limited_429: ["count>0"],
    // 429 responses should be fast (no backend processing)
    rejection_latency_ms: ["p(95)<50"]
  }
};

const GATEWAY_URL = __ENV.GATEWAY_URL || "http://localhost:4040";
const API_KEY = __ENV.API_KEY;
const ACCOUNT_ID = __ENV.ACCOUNT_ID || "00000000-0000-0000-0000-000000000000";

export default function () {
  const url = `${GATEWAY_URL}/v1/bank_account/${ACCOUNT_ID}`;
  const params = {
    headers: {
      Authorization: `Bearer ${API_KEY}`
    },
    tags: { name: "rate_limit_probe" }
  };

  const res = http.get(url, params);

  if (res.status === 429) {
    rateLimited.add(1);
    rateLimitRate.add(1);
    rejectionLatency.add(res.timings.duration);

    check(res, {
      "429 has Retry-After header": (r) =>
        r.headers["Retry-After"] !== undefined
    });
  } else {
    passed.add(1);
    rateLimitRate.add(0);

    check(res, {
      "non-429 is 200 or 404": (r) => r.status === 200 || r.status === 404
    });
  }

  // No sleep — maximum pressure to saturate the rate limiter
}

export function handleSummary(data) {
  const limited = data.metrics.rate_limited_429
    ? data.metrics.rate_limited_429.values.count
    : 0;
  const total = data.metrics.http_reqs
    ? data.metrics.http_reqs.values.count
    : 0;
  const pct = total > 0 ? ((limited / total) * 100).toFixed(1) : 0;

  console.log(`\n=== Rate Limit Saturation Summary ===`);
  console.log(`Total requests:    ${total}`);
  console.log(`Rate limited (429): ${limited} (${pct}%)`);
  console.log(`Passed through:    ${total - limited}`);

  if (limited === 0) {
    console.log(
      `\n⚠️  WARNING: No 429 responses — rate limiter may not be working!`
    );
  } else {
    console.log(`\n✅ Rate limiter is active and working under pressure.`);
  }

  return {};
}
