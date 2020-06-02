# Configuration

The kubelet can be configured via the command line, configuration file or
environment variables.

## Configuration values

| Command line | Environment variable | Configuration file | Description |
|--------------|----------------------|--------------------|-------------|
| -a, --addr   | KRUSTLET_ADDRESS     | addr               | The address on which the kubelet should listen |
| --data-dir    | KRUSTLET_DATA_DIR   | data_dir           | The path under which the kubelet should store data (e.g. logs, container images, etc.) |
| --hostname    | KRUSTLET_HOSTNAME   | hostname           | The name of the host where the kubelet runs. Pass this if the name in TLS certificate does not match the actual machine name |
| --max-pods   | MAX_PODS             | max_pods           | The maximum number of pods to schedule on the kubelet at any one time |
| -n, --node-ip | KRUSTLET_NODE_IP    | node_ip            | The IP address of the node registered with the Kubernetes master |
| --node-labels | NODE_LABELS         | node_labels        | The labels to apply to the node when it registers in the cluster. See below for format |
| --node-name   | KRUSTLET_NODE_NAME  | node_name          | The name by which to refer to the kubelet node in Kubernetes |
| -p, --port   | KRUSTLET_PORT        | port               | The port on which the kubelet should listen |
| --tls-cert-file | TLS_CERT_FILE     | tls_cert_file      | The path to the TLS certificate for the kubelet |
| --tls-private-key-file | TLS_PRIVATE_KEY_FILE     | tls_private_key_file      | The path to the private key for the TLS certificate |

## Node labels format

If you specify node labels on the command line or in an environment variable,
the format is a comma-separated list of `name=value` pairs. For example:

```
--node-labels mylabel=foo,myotherlabel=bar
```

If you specify node labels in the configuration file, the format is key-value
pairs. For example:

```json
{
    "node_labels": {
        "mylabel": "foo",
        "myotherlabel": "bar"
    }
}
```

## Configuration file location

The configuration file is located at `$HOME/.krustlet/config/config.json`.

**TODO: define how to override this.**

## Precedence

If you specify the same setting in multiple places - for example, both in
the configuration file and on the command line - then the precedence is:

* Command line flags take precedence over environment variables
* Environment variables take precedence over the configuration file

This allows you to conveniently override individual settings from a
configuration file, for example by writing `MAX_PODS=200 krustlet-wascc` or
`krustlet-wascc --max-pods 200`.

If you specify node labels in multiple places, the collections are _not_
combined: the place with the highest precedence takes effect and all others
are ignored.
