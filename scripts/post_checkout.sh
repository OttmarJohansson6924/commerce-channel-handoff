#!/bin/sh
set -eu

curl --fail-with-body \
  --request POST \
  --header 'content-type: application/json' \
  --data '{
    "request_id": "checkout-ord-1042-v1",
    "order_id": "ord-1042",
    "customer_name": "Ada Chen",
    "email": "ada@example.com",
    "phone": "+15550101420",
    "signup_channel": "email",
    "state": "checkout_confirmed"
  }' \
  http://127.0.0.1:3000/orders/update

