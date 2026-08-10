# Wi-Fi credentials

During initial setup, the modem (e.g. [LCW-007](LCW-007)) doesn't have access to the user's Wi-Fi network. It opens a SoftAP with a SSID of the format:

- during normal operation: `LGE_XXX_yyyy`. For example `LGE_AC2_1234`. The password for this network is `yyyy` repeated twice, so in this example it would be `12341234`
- in some other cases it may happen to be a string like `[LG_something]yyyy`, for example `[LG_Wall-Mount A/C]1234`. The password is fixed to `1111122222`. This SSID scheme is not understood by the official app, and I guess that it's not intentionally activated, just a bug. It showed up on my A/C immediately after power-on, even before pressing the magic "set up wi-fi" button sequence.

# Communications protocol variants

The app was found to support several versions of the setup protocol. Two variants are supported by rethink:

- [JSON-based + TLS](SetupProtocol%3AJSON) - supported by various Thinq2 devices
- [XML-based + TLS](SetupProtocol%3AXML) - supported by Thinq1 devices such as [WTDN3](Appliance%3AWTDN3)

The `rethink-setup` utility will automatically try both options.
