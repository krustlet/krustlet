# Configuration

The `kubelet` crate supports configuration via the command line, configuration file or
environment variables.

**NOTE:** Custom kubelets built using the `kubelet` crate can choose which of
these methods to support, or may choose to bypass `kubelet`'s built-in
configuration system in favour of their own. `krustlet-wascc` and
`krustlet-wasi` use standard configuration and support all configuration methods.

## Configuration values

| Command line | Environment variable | Configuration file | Description |
|--------------|----------------------|--------------------|-------------|
| -a, --addr   | KRUSTLET_ADDRESS     | listenerAddress    | The address on which the kubelet should listen |
| --data-dir   | KRUSTLET_DATA_DIR    | dataDir            | The path under which the kubelet should store data (e.g. logs, container images, etc.). The default is `$HOME/.krustlet` |
| --hostname   | KRUSTLET_HOSTNAME    | hostname           | The name of the host where the kubelet runs. Defaults to the hostname of the machine where the kubelet is running; pass this if the name in the TLS certificate does not match the actual machine name |
| --max-pods   | MAX_PODS             | maxPods            | The maximum number of pods to schedule on the kubelet at any one time. The default is 110 |
| -n, --node-ip | KRUSTLET_NODE_IP    | nodeIP             | The IP address of the node registered with the Kubernetes master. Defaults to the IP address of the kubelet hostname, as obtained from DNS |
| --node-labels | NODE_LABELS         | nodeLabels         | The labels to apply to the node when it registers in the cluster. See below for format |
| --node-name  | KRUSTLET_NODE_NAME   | nodeName           | The name by which to refer to the kubelet node in Kubernetes. Defaults to the hostname |
| -p, --port   | KRUSTLET_PORT        | listenerPort       | The port on which the kubelet should listen. The default is 3000 |
| --tls-cert-file | TLS_CERT_FILE     | tlsCertificateFile | The path to the TLS certificate for the kubelet. The default is `(data directory)/config/krustlet.crt` |
| --tls-private-key-file | TLS_PRIVATE_KEY_FILE | tlsPrivateKeyFile | The path to the private key for the TLS certificate. The default is `(data directory)/config/krustlet.key` |

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

By default, the configuration file is located at `$HOME/.krustlet/config/config.json`.
The `kubelet` crate does not define a common way to override this.  However,
custom kubelets built on `kubelet` may provide such a mechanism.

The `krustlet-wascc` and `krustlet-wasi` kubelets do not currently provide
a way to override the default location.

**TODO: should we build in a standard way of overriding the file location?**

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
