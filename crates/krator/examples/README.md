# Moose Example

This is a small sample application using most of the features `krator` and `krator_derive` have to offer.

## Run example

In order to use all features from the moose example, compile it with `admission-webhook`, `derive` and `krator-derive/admission-webhook` features enabled -- this is reflected through the aggregate feature `derive-admission-webhook`

    RUST_LOG=moose=info,krator=info cargo run --example=moose --features=derive-admission-webhook # with admission webhook
    RUST_LOG=moose=info,krator=info cargo run --example=moose --features=derive                   # w/o admission webhook

When the example is compiled and started it automatically installs the CRD into the Kubernetes cluster. If it is compiled _with the webhook features enabled_, it installs all necessary admission webhook resources at startup:

- the mutating webhook configuration (cluster wide)
- a service definition (into namespace `default`)
- a secret containing the certificate and the private key for serving the webhook (into namespace `default`)

all these resources are owned by the CRD, so deleting the CRD will basically delete everything.

When the example is starting up, follow the instructions printed out.

