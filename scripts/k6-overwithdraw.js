import http from 'k6/http';
import { check } from 'k6';

export let options = {
  vus: 10,
  duration: '5s',
};

const BASE_URL = __ENV.BASE_URL || 'http://localhost:3030';
const TOKEN = __ENV.TOKEN;
const ACCOUNT_ID = __ENV.ACCOUNT_ID;

export default function() {
  const url = `${BASE_URL}/v1/bank_account`;
  const payload = JSON.stringify({
    "Withdrawal": {
      "id": ACCOUNT_ID,
      "amount": {
        "amount": "100",
        "currency": "USD"
      }
    }
  });

  const params = {
    headers: {
      'Content-Type': 'application/json',
      'Authorization': `Bearer ${TOKEN}`,
    },
  };

  let res = http.post(url, payload, params);
  check(res, {
    'status is 200': (r) => r.status === 200,
  });
}
