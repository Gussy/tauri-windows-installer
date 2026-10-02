# Code signing

Authenticode signing identifies the publisher and protects the final executable against modification. Signing runs after PE resource, icon, version and manifest changes. A valid signature does not guarantee that SmartScreen will suppress a warning; Microsoft states that EV certificates no longer receive an automatic reputation bypass. See [SmartScreen reputation for developers](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation).

## Configuration

Resolution order is CLI `--sign-command`, plugin `signCommand`, then Tauri `bundle.windows.signCommand`. Structured Tauri commands retain their program and argument boundaries. `%1` is substituted in place, including inside an argument; when absent, the output filename is appended. Commands run directly as a program, so shell operators and environment-variable expansion require an explicit wrapper script.

```powershell
bundler -c tauri.conf.json -a app.exe --sign-command 'signtool sign /fd SHA256 /sha1 THUMBPRINT /tr http://timestamp.digicert.com /td SHA256 %1'
```

Use the certificate store or a wrapper that reads credentials securely. Avoid putting passwords directly in command-line configuration.

Structured Tauri configuration supports executable paths and arguments containing spaces:

```json
{
  "bundle": {
    "windows": {
      "signCommand": {
        "cmd": "C:\\Program Files\\Signer\\sign.exe",
        "args": ["--input=%1", "--timestamp", "http://timestamp.example.com"]
      }
    }
  }
}
```

For library calls:

```rust
use twi_core::SigningCommand;

let command = SigningCommand {
    program: "signtool.exe".into(),
    args: vec![
        "sign".into(), "/fd".into(), "SHA256".into(),
        "/sha1".into(), "THUMBPRINT".into(),
        "/tr".into(), "http://timestamp.digicert.com".into(),
        "/td".into(), "SHA256".into(), "%1".into(),
    ],
};
// Set BundleOptions.sign_command = Some(command).
```

The bundler rejects signer failure, missing certificate output and changed package resources. These structural checks do not establish certificate-chain trust. Validate the final executable with Windows Authenticode verification before distributing it:

```powershell
signtool verify /pa /all /v MyApp-setup.exe
Get-AuthenticodeSignature .\MyApp-setup.exe
```

## macOS / Linux signing

`osslsigncode` writes a separate output. Use a wrapper that replaces the input only after successful signing:

```sh
#!/usr/bin/env bash
set -euo pipefail
output=$(mktemp "${1}.signed.XXXXXX")
trap 'rm -f "$output"' EXIT
osslsigncode sign -certs "$CERT_PATH" -key "$KEY_PATH" \
  -ts http://timestamp.digicert.com -h sha256 -in "$1" -out "$output"
mv "$output" "$1"
```

Invoke it with `--sign-command ./scripts/sign.sh`. Verify the result on Windows as well as with `osslsigncode verify`.

## Timestamping and releases

Include a timestamp using signtool `/tr` with `/td SHA256`, or osslsigncode `-ts`, so certificate expiration does not invalidate an otherwise valid signing-time signature.

The release workflow can sign both the bare setup stub and the bundler. Configure repository variable `TWI_RELEASE_SIGN_PROGRAM` and secret `TWI_RELEASE_SIGN_ARGS` as a JSON argument array. The signer receives `%1` in place or a trailing filename. The stub is signed before embedding; the bundler is signed after compilation. Both outputs must then have a valid timestamped Authenticode signature.

Set repository variable `TWI_REQUIRE_RELEASE_SIGNATURE` to `true` to prevent unsigned releases. Without it signing is optional and provenance records the actual signature status. `tests/verify-release.ps1` additionally validates the embedded stub hash, ABI, schema, version and static MSVC runtime. No credentials or signing services are provisioned automatically.
