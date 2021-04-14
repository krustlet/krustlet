#!/bin/bash
case `uname` in
        Darwin)
            b64_opts='-b=0'
            export ENDPOINT=$(ipconfig getifaddr en0)
            ;;
        *)
            b64_opts='--wrap=0'
            export ENDPOINT="172.17.0.1"
esac

# remove the selector from the service so we can set an explicit endpoint to our local instance
kubectl patch service mooses-animals-com-admission-webhook --type="merge" -p '{"spec": {"selector": null}}'

cat<<EOF|kubectl apply -f -
---
apiVersion: v1
kind: Endpoints
metadata:
  name: mooses-animals-com-admission-webhook
  labels:
    app: mooses-animals-com-operator
subsets:
  - addresses:
      - ip: ${ENDPOINT}
    ports:
      - port: 8443
EOF
