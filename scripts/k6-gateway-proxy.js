/**
 * k6 Load Test: Gateway Proxy Pipeline
 *
 * Measures gateway overhead: API key resolution → rate limit check → JWT mint → reverse proxy → Core.
 * Target: GET /v1/bank_account/:id via Gateway (:4040)
 *
 * Usage:
 *   k6 run -e API_KEY=bk_live_... -e ACCOUNT_ID=<uuid> scripts/k6-gateway-proxy.js
 *
 * Environment variables:
 *   GATEWAY_URL  - Gateway URL (default: http://localhost:4040)
 *   API_KEY      - Valid API key (bk_live_...)
 *   ACCOUNT_ID   - Valid bank account UUID (use any existing account)
 */

import http from 'k6/http';
import { check, sleep } from 'k6';
import { Rate, Trend } from 'k6/metrics';

const gatewayOverhead = new Trend('gateway_overhead_ms');
const errorRate = new Rate('errors');

export const options = {
  stages: [
    { duration: '30s', target: 50 },   // Ramp up to 50 VUs
    { duration: '60s', target: 100 },   // Sustained 100 VUs
    { duration: '10s', target: 0 },     // Ramp down
  ],
  thresholds: {
    http_req_duration: ['p(95)<200'],   // 95th percentile < 200ms
    errors: ['rate<0.05'],               // Error rate < 5%
  },
};

const GATEWAY_URL = __ENV.GATEWAY_URL || 'http://localhost:4040';
const API_KEY = __ENV.API_KEY;
const ACCOUNT_ID = __ENV.ACCOUNT_ID;

export default function () {
  const url = `${GATEWAY_URL}/v1/bank_account/${ACCOUNT_ID}`;
  const params = {
    headers: {
      Authorization: `Bearer ${API_KEY}`,
    },
    tags: { name: 'gateway_proxy_get_account' },
  };

  const res = http.get(url, params);

  check(res, {
    'status is 200': (r) => r.status === 200,
    'response has id': (r) => {
      try {
        return JSON.parse(r.body).id !== undefined;
      } catch {
        return false;
      }
    },
  }) || errorRate.add(1);

  gatewayOverhead.add(res.timings.duration);
  sleep(0.1); // 100ms between requests per VU
}
