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
      "id": "9fcee205-8072-4cda-a4d3-42b4f0545b18",
      "amount": {
        "amount": "30",
        "currency": "TWD"
      }
    }
  });

  const params = {
    headers: {
      'Content-Type': 'application/json',
      'Authorization': 'Bearer eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJpc3MiOiJiYW5raWUiLCJzdWIiOiJqYXJ2aXMtYXBpIiwiYXVkIjoic2VydmljZSIsImV4cCI6MTc1NTQzNTYyMSwiaWF0IjoxNzIzODk5NjIxLCJzY29wZSI6WyJiYW5rLWFjY291bnQ6cmVhZCIsImJhbmstYWNjb3VudDp3cml0ZSIsImxlZGdlcjpyZWFkIl0sInRlbmFudF9pZCI6MX0.njnIZr1D4RF45hRDdAiC2CdXz6_7bOllZ3710I_guLw'
    },
  };

  let res = http.post(url, payload, params);
  check(res, {
    'status is 200': (r) => r.status === 200,
  });
}
