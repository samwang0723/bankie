import http from 'k6/http';
import { check } from 'k6';

export let options = {
  vus: 2, // number of virtual users
  duration: '1s', // duration of the test
};

export default function() {
  const url = 'http://localhost:3030/v1/bank_account';
  const payload = JSON.stringify({
    "Withdrawal": {
      "id": "dfb15f11-9c54-4935-b071-148ca2df73d1",
      "amount": {
        "amount": "100",
        "currency": "TWD"
      }
    }
  });

  const params = {
    headers: {
      'Content-Type': 'application/json',
      'Authorization': 'Bearer eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJpc3MiOiJiYW5raWUiLCJzdWIiOiJqYXJ2aXMtYXBpIiwiYXVkIjoic2VydmljZSIsImV4cCI6MTc1NTU2NTYzMCwiaWF0IjoxNzI0MDI5NjMwLCJzY29wZSI6WyJiYW5rLWFjY291bnQ6cmVhZCIsImJhbmstYWNjb3VudDp3cml0ZSIsImxlZGdlcjpyZWFkIl0sInRlbmFudF9pZCI6MX0.e2NP0iTucstXXWd7J9qwgOi5Bw-mkZQGKpaaA0mWlmo'
    },
  };

  let res = http.post(url, payload, params);
  check(res, {
    'status is 200': (r) => r.status === 200,
  });
}
