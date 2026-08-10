# Security

Various kinds of encryption are used throughout the process. What is important is that the device doesn't come with any kind of trust root from the factory. All keys and certificates are provided or created during the setup process - meaning that we can tell the device to trust our own keys.

During normal operation, the device trusts only the keys obtained during setup, so the communications can't be easily intercepted.

# Communications protocol

The modem listens for connections on TCP port 5500. It offers a TLS server with a self-signed certificate, the same for every device. The public key for this server is hardcoded in the [app](PieceOfCrapp) (side-note: you can convince the app to communicate with your server at this stage by either pulling the private key from the LCW's flash, or adding another key to the `res/raw/ha.bks` keystore)

On the application layer, a JSON-based protocol is used (with no delimiters, so splitting into packets is slightly inconvenient). Application-level packets look like:

```
{"type":"request", "cmd":"COMMAND", "data": { ... }}
{"type":"response", "cmd":"COMMAND", "data": { ... }}
```
## Common arguments

```
{"type":"request", "cmd":"COMMAND", "data": { "constantConnect":"Y" }}
```

Without `constantConnect`, the device will disconnect after completing the command.


## Commands:

### getDeviceInfo

Request: 
```
{"type":"request", "cmd": "getDeviceInfo", "data": { "publicKey": "pem-formatted public key", "constantConnect":"Y", "mqttServer":"ssl://rethink.lan:8884", "apiServer":"https://rethink.lan:4433" }}
```

- `publicKey` is a PEM-formatted string (with newlines and all) that the device will use to validate with the cloud service. The app passes the public key that most likely comes from the cloud, but we can easily provide a different one at this step.
- `apiServer` and `mqttServer` are optional, if not provided, the device will obtain these values by querying the common server (`https://common.lgthinq.com/route`).

TODO: check if providing mqttServer & apiServer actually circumvents all requests to common.lgthinq.com!

Response:
```
{"type":"response","cmd":"getDeviceInfo","data":{"protocolVer":"4.5","supportsWpa3":"Y","mdFota":"Y","demandType":"RTK_RTL8711am","mac":"d8:xx:xx:xx:xx:xx","uuid":"xxxxxxxx-xxxx-xxxx-xxxx-xxxx","encrypt_val":"base64-encoded-rsa-encrypted-token","deviceType":"401","modelName":"RAC_056905_WW","softwareVer":"690409","eepromChecksum":"4d81", "countryCode":"PL","modemVer":"clip_hna_v1.9.183","ch":"1","errorcodeDisplay":"0","remainingTime":"420","errorCode":"nb","errSSID":"abcdefgh","subErrorCode":"W011","extra":"CLP_RESET_REASON_WATCHDOG|7553711059013626529|94","factoryResetCode":"0","isReg":"N","agentVer":"1"}}
```

- `deviceType` matches the value from [this list](https://github.com/sampsyo/wideq/blob/511e342e5d3d5c3ae3c18d8fef7f922bf4c1f43c/wideq/client.py#L246)
- `encrypt_val` is created in the following way:
    1. A random 32-bit value is converted to a 8-character hex string
    2. sha256(device uuid) is appended
    3. the resulting 40-byte value is RSA-encrypted using the public key provided in the request
    4. the resulting 256-byte value is base64-encoded


### setDeviceInit

```
{"type":"request", "cmd": "setDeviceInit", "data": { "set":"true", "constantConnect":"Y"}}
```

This command sets some flag in the firmware that - I think - causes the module to discard the private key/certificate that it already had obtained and request a new one.


### setCertInfo

```
{"type":"request", "cmd": "setCertInfo", "data": {"otp": "48-character string","svccode":"SVC202","svcphase":"OP","constantConnect":"Y"}}'
```

- `otp` is a 48-character token provided by the app (retrieved from LG cloud, I guess). It will be used by the cloud to verify a request at a later stage.
- `svcphase` is very interesting. Normally the app sets it to `OP`. If set to `SC` or `QA` though, it activates an [UART console](LCW-007#debug-uart) on the wi-fi module.

### setApInfo

```
{"type":"request","cmd":"setApInfo","data":{"format":"B64","ssid":"base64-encoded SSID","security":"WPA2_PSK","cipher":"AES","password":"base64-encoded passphrase","constantConnect":"Y"}}
```

- `ssid` and `password` are base64-encoded
- I haven't checked other Wi-Fi encryption options

### releaseDev

```
{"type":"request","cmd":"releaseDev"}
```

This causes the device to close the SoftAP and connect to the network provided in the previous steps in attempt to provision the device with the cloud.

### Other commands

These names were extracted from [the module's flash](LCW-007)

- PERF_getDeviceInfo
- PERF_setCertInfo
- PERF_apScanList
- PERF_setApInfo
- formatDev
- cancelSetup
- trpTistestmode
- SVCBlackBoxRequest
- LQCModeStart
- LQCModeEnd
- LQCDataToProduct
- LQCChannel
- phoneToProduct
- req_publickey_concurrency
- resp_publickey_concurrency
- findDevice_concurrency
- liveCheck_concurrency
- liveCheckAck_concurrency
- confirm_concurrency
- confirmAck_concurrency
- apInfo_concurrency
- regState_concurrency
- start_concurrency
- ping
- pong
- PERF_setConfirm
- startDirectFota
- fotaInfo
- fotaPgmInfo
- fotaPgmSend
- fotaFirmwareMode
- fotaDownloadMode
- modemFotaStart
- getExtraModemLog
- resetDevice
- blackboxExt

# Summarized setup procedure

1. Activate SoftAP on the Wi-Fi module if needed (see appliance manual for details)
2. Connect to the SoftAP using the default [credentials](#wi-fi-credentials)
3. Connect to the device (IP: 192.168.120.254 in my case) on port 5500, with TLS.
4. Optional: send [setDeviceInit](#setDeviceInit)
5. Send [getDeviceInfo](#getDeviceInfo)
6. If you are the official app: obtain the public key  and `otp` token from the cloud
7. Send [setCertInfo](#setCertInfo)
8. Send [setApInfo](#setApInfo)
9. Send [releaseDev](#releaseDev)
10. The device connects to the Wi-Fi network and executes the [provisioning procedure](Thinq2CloudProtocol#provisioning)
