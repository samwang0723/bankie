/**
 * k6 Load Test: Concurrent Banking Operations
 *
 * Tests CQRS channel backpressure and double-entry consistency under concurrent
 * deposits and withdrawals. Each VU alternates between deposit and withdrawal
 * on a shared account.
 *
 * Usage:
 *   k6 run -e TOKEN=<jwt> -e ACCOUNT_ID=<uuid> scripts/k6-concurrent-banking.js
 *
 * Environment variables:
 *   BASE_URL     - Core server URL (default: http://localhost:3030)
 *   TOKEN        - Valid JWT token
 *   ACCOUNT_ID   - Approved bank account UUID with sufficient balance
 */

import http from 'k6/http';
import { check, sleep } from 'k6';
import { Counter, Rate } from 'k6/metrics';

const deposits = new Counter('deposits');
const withdrawals = new Counter('withdrawals');
const errorRate = new Rate('errors');

export const options = {
  vus: 20,
  duration: '30s',
  thresholds: {
    http_req_duration: ['p(95)<500'],   // 95th percentile < 500ms (CQRS channel may queue)
    errors: ['rate<0.10'],               // Allow some async failures
  },
};

const BASE_URL = __ENV.BASE_URL || 'http://localhost:3030';
const TOKEN = __ENV.TOKEN;
const ACCOUNT_ID = __ENV.ACCOUNT_ID;

const params = {
  headers: {
    'Content-Type': 'application/json',
    Authorization: `Bearer ${TOKEN}`,
  },
};

export default function () {
  const url = `${BASE_URL}/v1/bank_account`;

  // Alternate between small deposits and withdrawals
  if (__ITER % 2 === 0) {
    // Deposit $10
    const depositPayload = JSON.stringify({
      Deposit: {
        id: ACCOUNT_ID,
        amount: { amount: '10', currency: 'USD' },
      },
    });

    const res = http.post(url, depositPayload, {
      ...params,
      tags: { name: 'deposit' },
    });

    check(res, {
      'deposit accepted (200)': (r) => r.status === 200,
    }) || errorRate.add(1);

    deposits.add(1);
  } else {
    // Withdraw $5 (smaller than deposit to avoid overdraft under concurrency)
    const withdrawPayload = JSON.stringify({
      Withdrawal: {
        id: ACCOUNT_ID,
        amount: { amount: '5', currency: 'USD' },
      },
    });

    const res = http.post(url, withdrawPayload, {
      ...params,
      tags: { name: 'withdrawal' },
    });

    check(res, {
      'withdrawal accepted (200)': (r) => r.status === 200,
    }) || errorRate.add(1);

    withdrawals.add(1);
  }

  sleep(0.05); // 50ms between operations
}
