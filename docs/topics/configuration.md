# Configuration

The `kubelet` crate supports configuration via the command line, configuration file or
environment variables.

**NOTE:** Custom kubelets built using the `kubelet` crate can choose which of
these methods to support, or may choose to bypass `kubelet`'s built-in
configuration system in favour of their own. `krustlet-wascc` and
`krustlet-wasi` use standard configuration and support all configuration methods.

**NOTE:** Certain flags must be handled at the provider or custom kubelet level. If you
are building a custom kubelet using the `kubelet` crate, please see the "Notes to 
kubelet implementers" section below.

## Configuration values

| Command line       | Environment variable      | Configuration file | Description                                                                                                                                                                                            |
|--------------------|---------------------------|--------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| -a, --addr         | KRUSTLET_ADDRESS          | listenerAddress    | The address on which the kubelet should listen                                                                                                                                                         |
| --data-dir         | KRUSTLET_DATA_DIR         | dataDir            | The path under which the kubelet should store data (e.g. logs, container images, etc.). The default is `$HOME/.krustlet`                                                                               |
| --hostname         | KRUSTLET_HOSTNAME         | hostname           | The name of the host where the kubelet runs. Defaults to the hostname of the machine where the kubelet is running; pass this if the name in the TLS certificate does not match the actual machine name |
| --max-pods         | MAX_PODS                  | maxPods            | The maximum number of pods to schedule on the kubelet at any one time. The default is 110                                                                                                              |
| -n, --node-ip      | KRUSTLET_NODE_IP          | nodeIP             | The IP address of the node registered with the Kubernetes master. Defaults to the IP address of the kubelet hostname, as obtained from DNS                                                             |
| --node-labels      | NODE_LABELS               | nodeLabels         | The labels to apply to the node when it registers in the cluster. See below for format                                                                                                                 |
| --node-name        | KRUSTLET_NODE_NAME        | nodeName           | The name by which to refer to the kubelet node in Kubernetes. Defaults to the hostname                                                                                                                 |
| -p, --port         | KRUSTLET_PORT             | listenerPort       | The port on which the kubelet should listen. The default is 3000                                                                                                                                       |
| --cert-file        | KRUSTLET_CERT_FILE        | tlsCertificateFile | The path to the TLS certificate for the kubelet. The default is `(data directory)/config/krustlet.crt`                                                                                                 |
| --private-key-file | KRUSTLET_PRIVATE_KEY_FILE | tlsPrivateKeyFile  | The path to the private key for the TLS certificate. The default is `(data directory)/config/krustlet.key`                                                                                             |
| --x-allow-local-modules | KRUSTLET_ALLOW_LOCAL_MODULES | allowLocalModules | If true, the kubelet should recognise references prefixed with 'fs' as indicating a filesystem path rather than a registry location. This is an experimental flag for use in development scenarios where you don't want to repeatedly push your local builds to a registry; it is likely to be removed in a future version when we have a more comprehensive toolchain for local development. |

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

## Notes to kubelet implementers

Some flags require you to support them in your provider or main code - they are
not implemented automatically by the kubelet core. These flags are as follows:

* `--bootstrap-file` - should be passed to `kubelet::bootstrap` if you use the
  bootstrapping feature
* `--data-dir` - this should be used to construct the `FileStore` if you use one
* `--x-allow-local-modules` - if specified you should compose a `FileSystemStore`
  onto your normal store

See the `krustlet-wasi.rs` file for examples of how to honour these flags.

If you can't honour a flag value in your particular scenario, then you should
still check for it and return an error, rather than silently ignoring it.
