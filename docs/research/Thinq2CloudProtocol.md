Once connected to the user's Wi-Fi network ThinQ2 devices communicate with LG's cloud service using HTTPs and MQTTs. No incoming connections are accepted.

# Provisioning

The first steps after connecting to the Wi-Fi network are called "provisioning". During this process, the modem creates a key/certificate pair that it will use to authenticate itself with the cloud service.

Steps:

## Find mqttServer & apiServer

TODO: find out if this step is skipped when mqttServer & apiServer are provided using the [setup protocol](SetupProtocol%3AJSON#getDeviceInfo).

The device sends a https request to `https://comon.lgthinq.com/route` (this URL is stored in the firmware and not modified in normal operation). **any HTTPS certificate is accepted**.

Response: JSON
```
{"resultCode": "0000", "result": {"apiServer": "https://eic-common.lgthinq.com:443", "mqttServer": "ssl://a3phael99lf879.iot.eu-west-1.amazonaws.com:8883"}}
```

## Request certificates from apiServer

1. Request: ```https://common.lgthinq.com/route/certificate```

   Response: ```{"resultCode": "0000", "result": ["common-server", "aws-iot"]}```

2. Request: ```https://common.lgthinq.com/route/certificate?name=common-server```

   Response: ```{"resultCode": "0000", "result": {"certificatePem": "pem-formatted certificate"}}```

   - `certificatePem` will be used as a trusted certificate when connecting to the apiServer

3. Request: ```https://common.lgthinq.com/route/certificate?name=aws-iot```

   Response: ```{"resultCode": "0000", "result": {"certificatePem": "pem-formatted certificate"}}```

   - `certificatePem` will be used as a trusted certificate when connecting to the mqttServer

## Submit Certificate Signing Request

Request: ```POST https://apiServer/device/xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx/certificate```
- the request body contains the pem-formatted CSR

Response: ```{"resultCode":"0000", "result":{"certificatePem":"pem-formatted certificate"}}```
- `certificatePem` must be used by the device, along with its private key to authenticate with the mqttServer

## MQTT provisioning

1. Connect to `mqttServer`
   - Validate the server certificate against the certificate received from `/route/certificate?name=aws-iot`
   - Use the client certificate obtained in the [previous step](#submit-certificate-signing-request)
   - Use the device uuid as the MQTT clientId
2. Subscribe to `lime/devices/xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` (device topic)
3. Publish a message on `topic_prov` (default: `clip/provisioning/devices/xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`)
   ```
   {"mid":945658,"did":"xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx","kind":"RAC_056905_WW","cmd":"preDeploy","rssi":-56,"fs":"idle","data":
   {"appInfo":"modelName":"RAC_056905_WW","modelLanguage":"PL","softVer":"690409","ruleVer":"2.0.11","countryCode":"PL",
   "subCountryCode":"PL","appVersion":"clip_hna_v1.9.183","modemType":"RTK_RTL8711am","regionalCode":"eic","timezone":"+0100",
   "svcCode":"SVC202","HomeApSsid":"thisismyssid","DeviceType":"","ruleEngine":"y","protocolVer":"1","oneshot":"y","size":1572864,
   "fwUpgradeInfo":{"upgSched":{"cmd":"none","upgUtc":"0"}}},"platformInfo":{"provisioningKey":"RAC_056905_WW","version":
   "clip_v2.00.15.05-RTK_RTL8711am-SDK-8-RELEASE"}},"type":0}
   ```
4. MQTT server responds with a message on the device topic:
   ```
   {"did":"xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx","mid":1700263891876,"cmd":"completeProvisioning","type":0,"data":
   {"result":0,"host":"message","appInfo":{"host":"message","publication":{
   "message":"$aws/rules/clip_http_action_rule/clip/devices/xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx/message",
   "provisioning":"$aws/rules/clip_provisioning_rule/clip/provisioning/devices/xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"}},
   "provisioningType":"preDeploy","deployInterval":600}}
   ```

   - `message` specifies the MQTT topic that the device will use to send its regular messages (`topic_message`)
   - `provisioning` specifies the MQTT topic that the device will use to send its provisioning messages (`topic_prov`)

     This topic will replace `topic_prov` in the module's flash. On subsequent setup attempts, the new topic will be used. When implementing a substitute for the cloud service, be aware that supplying a non-default `topic_prov` may make the device persistently incompatible with the official cloud until the correct `topic_prov` is configured again via this process or using the [debug UART shell](LCW-007#debug-uart).

5. Publish a message on `topic_message`
   ```
   {"mid":1899923,"did":"xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx","kind":"RAC_056905_WW","cmd":"completeProvisioning_ack","rssi":-46,"fs":"idle","data":null,"type":1}
   ```

   When this point is reached the provisioning is considered complete. The device stores a `registered` flag in flash and will remember the Wi-Fi & cloud configuration until manually reset.

# Subsequent connection setup

When the device connects to the MQTT server but the provisioning step has already been completed, the last three steps are repeated, with a slight difference:

1. Publish a message on `topic_prov`
   
   The contents are the same as during provisioning, but the `cmd` attribute is set to `deploy`.

2. MQTT server responds with a message on the device topic:

   The contents are the same as during provisioning, but the `provisioningType` is set to `deploy`.

3. Publish a message on `topic_message`

   The contents are the same as during provisioning.

# MQTT data exchange

## device-to-cloud messages

All messages at this stage are published to `topic_message`

### req_timesync

Request: ```{"mid":75383,"did":"xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx","kind":"RAC_056905_WW","cmd":"req_timesync","rssi":-48,"fs":"idle","data":null,"type":1}```

Response: unknown

### device_packet

```{"mid":109239,"did":"xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx","kind":"RAC_056905_WW","cmd":"device_packet","rssi":-48,"fs":"idle","data":"000004000000870204690B8CA036458CD059ACE001DE6AC9","type":1}```

- `data` is a hex-encoded byte sequence.

   See [TLV protocol](TLVProtocol) and [AABB Protocol](AABBProtocol) for details.

## cloud-to-device messages

All messages are published to the `lime/devices/xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` topic

### packet

```{"did":"xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx","mid":1700432732556,"cmd":"packet","type":1,"data":"00010400000065020201027D416A0D"}```

- `data` is a hex-encoded byte sequence.

   See [TLVProtocol] and [AABBProtocol] for details.
