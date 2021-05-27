$token_id = -join ((48..57) + (97..122) | Get-Random -Count 6 | ForEach-Object { [char]$_ })
$token_secret = -join ((48..57) + (97..122) | Get-Random -Count 16 | ForEach-Object { [char]$_ })

$expiration = (Get-Date).ToUniversalTime().AddHours(1).ToString("yyyy-MM-ddTHH:mm:ssZ")

@"
apiVersion: v1
kind: Secret
metadata:
  name: bootstrap-token-${token_id}
  namespace: kube-system
type: bootstrap.kubernetes.io/token
stringData:
  auth-extra-groups: system:bootstrappers:kubeadm:default-node-token
  expiration: ${expiration}
  token-id: ${token_id}
  token-secret: ${token_secret}
  usage-bootstrap-authentication: "true"
  usage-bootstrap-signing: "true"
"@ | kubectl.exe apply -f -

if (-not (Test-Path env:CONFIG_DIR)) { 
  $env:CONFIG_DIR = '$HOME\.krustlet\config'
}
$config_dir = $env:CONFIG_DIR

mkdir $config_dir -ErrorAction SilentlyContinue > $null

if (!$env:FILE_NAME -or -not (Test-Path $env:FILE_NAME)) {
  $file_name = "bootstrap.conf"
}
else {
  $file_name = env:FILE_NAME
}

# Helpful script taken from the armory docs: https://docs.armory.io/spinnaker-install-admin-guides/manual-service-account/
# and modified to suit our needs

$context = kubectl config current-context
$new_context = "tls-bootstrap-token-user@kubernetes"
$kubeconfig_file = "$config_dir\$file_name"
$token_user = "tls-bootstrap-token-user"
$token = "$token_id.$token_secret"


try {
  # Create a full copy
  kubectl config view --raw > "$kubeconfig_file.full.tmp"

  # Switch working context to correct context
  kubectl --kubeconfig "$kubeconfig_file.full.tmp" config use-context "$context"

  # Minify
  kubectl --kubeconfig "$kubeconfig_file.full.tmp" config view --flatten --minify >"$kubeconfig_file.tmp"

  # Rename context
  kubectl config --kubeconfig "$kubeconfig_file.tmp" rename-context "$context" "$new_context"

  # Create token user
  kubectl config --kubeconfig "$kubeconfig_file.tmp" set-credentials "$token_user" --token "$token"

  # Set context to use token user
  kubectl config --kubeconfig "$kubeconfig_file.tmp" set-context "$new_context" --user "$token_user"

  # Flatten/minify kubeconfig
  $content = kubectl config --kubeconfig "$kubeconfig_file.tmp" view --flatten --minify

  [IO.File]::WriteAllLines($kubeconfig_file, $content)
}

finally {
  Remove-Item -Force "$kubeconfig_file.full.tmp"
  Remove-Item -Force "$kubeconfig_file.tmp"
}

