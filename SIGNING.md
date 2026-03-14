# Code Signing

Code signing your setup executable ensures Windows SmartScreen doesn't block your installer and gives users confidence the file hasn't been tampered with.

Signing happens **after** the bundler finishes building the PE — all resource embedding, icon injection, and manifest writing complete first, then the sign command runs on the final output file.

## Configuration

You can provide a sign command in two ways. The CLI flag takes precedence over the plugin config.

### CLI flag

```bash
bundler -c tauri.conf.json -a target/release/myapp.exe -t "My App" \
  --sign-command "signtool sign /fd SHA256 /f cert.pfx /p password"
```

### Plugin config (tauri.conf.json)

```json
{
  "plugins": {
    "tauri-windows-installer": {
      "signCommand": "signtool sign /fd SHA256 /f cert.pfx /p password"
    }
  }
}
```

The bundler parses the command string using shell quoting rules and appends the output file path as the last argument. For example, if the sign command is `signtool sign /fd SHA256` and the output is `MyApp-setup.exe`, the bundler runs:

```
signtool sign /fd SHA256 MyApp-setup.exe
```

## Examples

### signtool (Windows SDK)

Using a PFX certificate file:

```bash
--sign-command "signtool sign /fd SHA256 /f path/to/cert.pfx /p YOUR_PASSWORD /tr http://timestamp.digicert.com /td SHA256"
```

Using a certificate from the Windows certificate store:

```bash
--sign-command "signtool sign /fd SHA256 /sha1 THUMBPRINT /tr http://timestamp.digicert.com /td SHA256"
```

### Azure Trusted Signing

```bash
--sign-command "signtool sign /fd SHA256 /tr http://timestamp.acs.microsoft.com /td SHA256 /dlib Microsoft.Trusted.Signing.Client/bin/x64/Azure.CodeSigning.Dlib.dll /dmdf metadata.json"
```

### osslsigncode (cross-platform)

For signing from macOS or Linux:

```bash
--sign-command "osslsigncode sign -certs cert.pem -key key.pem -ts http://timestamp.digicert.com -h sha256 -in"
```

Note: `osslsigncode` uses `-in <file>` rather than a trailing positional argument. Since TWI appends the file path as the last argument, use `-in` as the final flag and the path will follow it naturally.

### Custom script

Wrap complex signing logic in a script:

```bash
--sign-command "./scripts/sign.sh"
```

The script receives the file path as `$1`:

```bash
#!/bin/bash
set -euo pipefail
signtool sign /fd SHA256 /f "$CERT_PATH" /p "$CERT_PASSWORD" \
  /tr http://timestamp.digicert.com /td SHA256 "$1"
```

## Timestamping

Always include a timestamp server (`/tr` for signtool, `-ts` for osslsigncode). Without it, the signature expires when the certificate does.

Common timestamp servers:
- `http://timestamp.digicert.com`
- `http://timestamp.sectigo.com`
- `http://timestamp.acs.microsoft.com` (Azure Trusted Signing)

## Testing with a self-signed certificate

For development and testing:

```powershell
# Create a self-signed code signing certificate
$cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject "CN=TWI Test" -CertStoreLocation Cert:\CurrentUser\My

# Export to PFX
$password = ConvertTo-SecureString -String "test123" -Force -AsPlainText
Export-PfxCertificate -Cert $cert -FilePath test-cert.pfx -Password $password

# Sign
bundler -c tauri.conf.json -a app.exe -t "My App" \
  --sign-command "signtool sign /fd SHA256 /f test-cert.pfx /p test123"
```

Self-signed certificates will still trigger SmartScreen warnings. To avoid SmartScreen, you need a certificate from a trusted CA or an EV certificate.

## SmartScreen

Windows SmartScreen uses two signals to decide whether to warn users:

1. **Signature** — Is the file signed with a certificate from a trusted CA?
2. **Reputation** — Has this publisher signed enough files that users have installed without issues?

An EV (Extended Validation) certificate bypasses reputation requirements and removes SmartScreen warnings immediately. Standard OV (Organization Validation) certificates build reputation over time.
