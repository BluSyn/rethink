This page describes the TLV-based packet format as used by the "LG DualCool" AC.

# MQTT layer

## device-to-cloud messages

The packets published by the modem have a two-byte header. If the header is `0000`, the remaining bytes describe a packet received from the appliance via UART.
Otherwise, the contents were generated locally on the "modem", purpose unknown.

## cloud-to-device messages

The first two bytes determine wheter the payload is to be sent to the UART interface, or is destined to the "modem" itself. I don't know the exact conditions, but I'm only interested in the UART packets, they are formatted as follows:

- the first byte (the debug output labels it "a") is set to `01` if reliable delivery is required, `00` otherwise. The "reliable" packets will be retransmitted by the modem a few times if an acknowledgement is not received from the device.
- the second byte (the debug output labels it "s") is set to `01` if the acknowledgement is to be forwared to the cloud, `00` otherwise
- remaining bytes are forwarded unchanged to the device (although I think that the CRC checksum must be valid)

# UART framing format

An example packet on the UART interface looks like this:
```
byte-index: 00 01 02 03 04 05 06 07 08 .....
            04 00 00 00 65 01 01 01 02 AA 43 E6 CD
```

- byte0 - usually `04`; `A5` triggers a different code path labelled "EO packet"
- byte1..byte3 - unknown, always all-zero
- byte4: `65` on modem->appliance messages, `87` on appliance->modem messages
- byte5: - unknown, usually 1 or 2
  For appliance->modem messages, the following code paths have been identified:
  - 0x01 or 0x02 - behaviour depends on byte6:
    - byte6=0x01 - an ACK will be sent in response to this. This value is used when querying the device
    - byte6=0x02 - this value is used when sending commands to the device
    - byte6=0x03
    - byte6=0x04
    - byte6=0x10 - is an ACK packet. byte5 and byte7 mirror the request's bytes, and the payload is empty
    - byte6=0x1f
    - byte6=0x21
    - byte6=0x22
  - between 0x3 and 0x66 - code path labelled "SUPERSET"
  - between 0xf0 and 0xf9 trigger a different code path mentioning an "extended tlv" format
- byte6 - depends on byte5, see above
- byte7 - sometimes this looks like a sequence number, increasing between requests, echoed on responses
- byte8 - length of payload
- byte9+ - payload
- last two bytes - crc16 (xmodem variant) checksum of all preceding bytes

# [TLV](https://en.wikipedia.org/wiki/Type%E2%80%93length%E2%80%93value) payload

The payload (in most cases) seems to follow a type-length-value structure. The first 12 bits indicate the type ID, the next 2 bits are the payload length (0..3). If payload length is zero, the payload is encoded in the low 4 bits of the second byte. Otherwise, the low 4 bits are ignored, and the payload is read from subsequent bytes.

See the following ASCII art for reference:
```
MSB  LSB|
  byte0 |  byte1 | byte2..4
76543210|76543210|
             ^^^^----------- value (if length == 0)
                   ^^^^^^^^- value (if length > 0)
           ^^--------------- length (0..3)
^^^^^^^^^^^----------------- type
```

<details><summary>Here is some annotated output from the <a href="LCW-007#debug-uart">debug console</a> captured while parsing a long TLV message (click to expand)</summary>

```
[04 00 00 00 87 02 01 9F 49 B0 01 B0 50 57 B0 A0 01 7C B0 C1 B1 03 B3 06 B3 4F B4 C7 B5 82 B5 41 B5 43 B6 A0 4D 81 B6 F0 69 04 09 B7 01 BC 40 BD 47 B5 C0 B6 10 24 B6 46 B5 C1 B6 00 B6 46 B5 C2 B6 00 B6 46 B5 C4 B6 10 2C B6 46 B5 C6 B6 10 2C B6 46 5B C5 ]
@@=======================84

B001 tlv t=704 l=0 v=1
B05057 tlv t=705 l=1 v=87
B0A0017C tlv t=706 l=2 v=380
B0C1 tlv t=707 l=0 v=1
B103 tlv t=708 l=0 v=3
B306 tlv t=716 l=0 v=6
B34F tlv t=717 l=0 v=15
B4C7 tlv t=723 l=0 v=7
B582 tlv t=726 l=0 v=2
B541 tlv t=725 l=0 v=1
B543 tlv t=725 l=0 v=3
B6A04D81 tlv t=730 l=2 v=19841
B6F0690409 tlv t=731 l=3 v=6882313
B701 tlv t=732 l=0 v=1
BC40 tlv t=753 l=0 v=0
BD47 tlv t=757 l=0 v=7
B5C0 tlv t=727 l=0 v=0          
B61024 tlv t=728 l=1 v=36
B646 tlv t=729 l=0 v=6
B5C1 tlv t=727 l=0 v=1
B600 tlv t=728 l=0 v=0
B646 tlv t=729 l=0 v=6
B5C2 tlv t=727 l=0 v=2
B600 tlv t=728 l=0 v=0
B646 tlv t=729 l=0 v=6
B5C4 tlv t=727 l=0 v=4
B6102C tlv t=728 l=1 v=44
B646 tlv t=729 l=0 v=6
B5C6 tlv t=727 l=0 v=6
B6102C tlv t=728 l=1 v=44
B646 tlv t=729 l=0 v=6
```

</details>

The meaning of various type/value pairs depend on the device. For example, [LG DualCool AC](Appliance%3ARAC_056905_WW).

# Bugs

My AC unit intermittently sends malformed packets. An example sequence is below:
```
0000040000008702045C767E447DC17E837F502B7F9028C846C89064C8C08340838083C0868086C0870087C0894088408A038A50598A808CA003828CD038ACD03CD540D580C900CAD083CB105FCB40CB8CCBC0CC10CCCC5043CC905F8B40BF600471BFE004E3BFA00471C0200378BE5084BE90871B00BED043C300C340C0C0C380D1D4
0301040000008701100000EC3C
0000040000008702045D767E447DC17E837F502B7F9028C846C89064C8C08340838083C0868086C0870087C0894088408A038A50598A808CA003828CD038ACD03CD540D580C900CAD083CB105FCB40CB8CCBC0CC10CCCC5043CC905F8B40BF600471BFE004E30000000000000000000000BE90871B00BED043C300C340C0C0C380AC03
```

Note that the third packet is almost identical to the first, save for a different sequence number (5D instead of 5C), and a bunch of bytes replaced by 00. Note that the CRC is CORRECT!

I don't have any explanation for this other than a software bug either in the modem or the AC firmware itself. There is about 25-30% possibility that such packets will pass TLV parsing correctly. I don't yet have any ideas on how to protect against this.
