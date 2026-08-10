This page describes the fixed-format packets as used by various ThinQ devices. The format is tentatively named "AABB" based on the fixed first/last bytes of each packet.

# MQTT layer

This protocol was not yet analysed on the UART layer, so all the observations will refer to the packet format as exposed on the MQTT layer.

## Basic frame format

All packets, both sent & received by the device, follow a specific format. Example:

```
AA16F0263A03FF040100000000000300000000004FBB
```

- the first byte is always 0xAA
- the second byte is either 0xFF, or equal to the total length of the packet (including both AA & BB bytes)
- the second-to-last byte is a checksum
- the last byte is always 0xBB

## Checksum

The checksum is computed as a simple arithmetic sum of all preceding bytes (including AA), modulo 256, xor 0x55.

## Contents

The packets don't appear to follow a general format (contrary to the ["TLV" protocol](TLVProtocol)). See each specific device for details.
