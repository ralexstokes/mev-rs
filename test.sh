#! /bin/bash

echo "> status check"
curl -s -X POST -d '{"jsonrpc": "2.0", "method":"builder_status", "params": null, "id": 13378}' -H "Content-Type: application/json"  http://localhost:18550 | jq .
echo ""
echo "> validator registration"
curl -s -X POST -d '{"jsonrpc": "2.0", "method":"builder_registerValidatorV1", "params": {"fee_recipient":"0x0000000000000000000000000000000000000000","gas_limit":"0","timestamp":"0","public_key":"0xc00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"}, "id": 13379}' -H "Content-Type: application/json"  http://localhost:18550 | jq .
echo ""
echo "> fetching bid"
curl -s -X POST -d '{"jsonrpc": "2.0", "method":"builder_getHeaderV1", "params": {"slot": "3752095", "public_key": "0xc00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000", "parent_hash": "0x3030303030303030303030303030303030303030303030303030303030303030"}, "id": 13388}' -H "Content-Type: application/json"  http://localhost:18550 | jq .
echo ""
echo "> accepting bid"
curl -s -X POST -d '{"jsonrpc": "2.0", "method":"builder_getPayloadV1", "params": {"message": {}, "signature": ""}, "id": 13389}' -H "Content-Type: application/json"  http://localhost:18550 | jq .
