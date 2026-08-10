This page is a practical guide for contributors who want to add support for a new appliance to rethink. "Support" here means translating a device's binary protocol into Home Assistant entities, so that the appliance can be monitored and controlled without the LG cloud.

The general approach is reverse-engineering: get the device talking, watch its traffic, figure out which bytes mean what, and encode that knowledge in a device class. After the one-time [Setup](#setup), you can follow the [Manual workflow](#manual-workflow) or the [LLM-assisted workflow](#llm-assisted-workflow) - both reach the same place, and you can mix them. The steps are roughly ordered, but in practice you'll loop between them.

# Guidelines

- The primary goal is to map the device protocol to Home Assistant with little or no extra logic. Expose what the device reports and accepts, faithfully.
- Do not implement behaviour such as timers, schedules or safety locks. The user can configure these in Home Assistant, where they belong.
- If the device's internal safety features can be circumvented, do not do so intentionally.
- LLM-generated code is fine — as long as you understand it and can provide a rationale for the decisions taken. Unexplained or unreviewed generated code will not be accepted.

# Setup

## Prerequisites

- A working `rethink-cloud` installation — see the [installation guide](Installing-rethink‐cloud).
- The device's **ThinQ Model ID** (e.g. `RAC_056905_WW`, `2REF11EIDA__4`). This is printed during provisioning and is the key used to register a device class in the code.
- Ideally, an LG ThinQ account and the official app, so you can use [bridge mode](Bridge) and observe how LG's own cloud handles the device. This is the single most valuable reverse-engineering aid.
- Find or create an [issue](https://github.com/anszom/rethink/issues) for the unsupported device. This will allow other users to see that you're working on it.

## Provision the device

Run the device through the normal provisioning ("setup") process so that it connects to `rethink-cloud`. See the [setup protocol](SetupProtocol) and the installation guide.

**Provisioning is largely protocol-agnostic** — it usually succeeds even for devices rethink does not yet translate, because it only configures Wi-Fi and cloud endpoints. So you can provision an unsupported device and still observe its traffic.

If provisioning fails for your device, see [issue #58](https://github.com/anszom/rethink/issues/58).

Once provisioning completes, the device should appear in the [web management panel](Bridge#device-setup) (default port `44401`).

## Bridge the device to the official cloud

Enable [bridge mode](Bridge) for the device. In bridge mode, rethink forwards the device's traffic to the real LG cloud while still letting you observe every packet and inject new ones. This works even if the device's protocol is not supported yet.

In the bridge mode, the official LG app keeps working, so you can **toggle real settings and issue real commands** and watch what travels over the wire. Each packet sent to/from the device can be observed in the management panel and rethink-cloud logs.

# Manual workflow

With the device provisioned and bridged, this is the classic loop: identify the protocol, watch traffic while you operate the device, figure out the fields, and encode them in a device class.

## Identify the protocol variant

ThinQ2 devices have been observed to use one of two binary formats. Determine which one your device uses by inspecting the packets in the management panel's device monitor, or in the logs (enable the `incoming` log topic in `config.json`).

- **[TLV protocol](TLVProtocol)** — type/length/value-encoded fields, framed with a CRC16. Used e.g. by the [DualCool AC](Appliance%3ARAC_056905_WW). Extend [`TLVDevice`](#tlv-devices) in code.
- **[AABB protocol](AABBProtocol)** — fixed-layout packets bracketed by `0xAA`…`0xBB` with a simple checksum. Used e.g. by the [fridges](Appliance%3A2REF11EIDA__4) and newer washers. Extend `AABBDevice` in code.

ThinQ1 devices (like the [WTDN3 washer](Appliance%3AWTDN3)) use the older XML-style protocol and extend `HADevice` directly.

For **TLV** devices, the [`packet-parser`](https://github.com/anszom/rethink/blob/master/tools/packet-parser.ts) utility is the main tool for decoding traffic. Run it against the rethink MQTT broker to live-decode every packet a device sends:

```
npx tsx tools/packet-parser.ts <mqtt-hostname[:port]> <device-uuid>
```

or decode a single captured hex message offline:

```
npx tsx tools/packet-parser.ts -message <hex>
```

It prints each TLV element as `t=0x… l=… v=0x… (decimal)`. For **AABB** devices there's no generic decoder — you compare raw hex packets by hand (the existing [appliance pages](Appliance%3A2REF11EIDA__4) show the byte-table format that works well for documenting them).

## Find the closest existing device and use it as a skeleton

Before writing anything, look at the device classes in [`rethink/cloud/devices/`](https://github.com/anszom/rethink/tree/master/cloud/devices). The protocols for similar devices usually overlap significantly. Pick the most similar supported appliance, and copy it as a starting point. Common code can later be extracted to shared helpers like `fridge_common.ts` and `washer_common.ts`.

Good reference points:

- TLV air conditioner: `RAC_056905_WW.ts`
- AABB fridge: `2RES1VE600FWC.ts` (with `fridge_common.ts`)
- AABB washer/dryer: `F_V8_Y___W.B_2QEUK.ts` (with `washer_common.ts`)
- ThinQ1 washer: `WTDN3.ts`

Register your new class in [`cloud/ha_bridge.ts`](https://github.com/anszom/rethink/blob/master/cloud/ha_bridge.ts) by importing it and adding it to `t1deviceTypes` or `t2deviceTypes`, keyed by the device's Model ID. If two models turn out to be exactly compatible, you can map the same class under several IDs.

### TLV devices

`TLVDevice` provides a field-definition system: each call to `addField()` maps a TLV type ID to a Home Assistant property with read/write transforms, and the base class handles capability queries, periodic re-querying, and command framing for you. This is usually much less code than an AABB device.

### AABB devices

`AABBDevice` gives you `processAABB(buf)` for incoming packets and `send(buf)` for outgoing ones; you parse and build the fixed byte layouts yourself (see `2RES1VE600FWC.ts` for a compact example, including publishing HA entities via `setConfig()` and reacting to `setProperty()`).

Many AABB devices follow a specific scheme where updates are sent in a format consisting of a fixed-format binary "status block" prefixed with a small header.
- for writes, the "status block" is sent once, with unchanged fields set to 0xFF
- for notifications, the block is sent twice, first the old state, then the new one. It suffices to simply parse the new state

## Map state updates (device → cloud)

With the device bridged and the parser running, **toggle settings directly on the appliance** (buttons, dials, doors) and watch which fields change. This tells you how to *read* the device's state.

- For TLV devices, note which `t=` IDs change and how their values map to the physical state.
- For AABB devices, diff the raw packets before and after each change to locate the relevant byte(s).

Encode each finding as a property your device class publishes to Home Assistant.

## Map commands (cloud → device)

Now do the reverse: **change settings in the official LG app** and capture the commands the cloud sends down to the device. This tells you how to *write* state.

Match each app action to the resulting packet, then implement `setProperty()` (or the equivalent TLV write transform) so that Home Assistant commands produce the same bytes. Most devices also need an initial "query" packet before they report anything — e.g. the fridges send `F0ED…` on startup (see the device classes' `start()` method).

## Cross-check against the LG cloud's own parsing

For the trickiest fields, it helps to see how LG's cloud *interprets* a value, not just how the app displays it. Two complementary techniques:

- **Observe** — run [`lgcloud-monitor.ts`](https://github.com/anszom/rethink/blob/master/tools/lgcloud-monitor.ts). It logs into the official cloud the way the app does and prints the real-time MQTT notifications the cloud emits about your devices, so you can see the cloud's decoded interpretation of each state update.
- **Inject** — use the management panel's packet-injection feature (or [`packet-sender`](https://github.com/anszom/rethink/blob/master/tools/packet-sender.ts) / `packet-sender-device.ts`) to send *modified* device-side packets while bridged, then watch via `lgcloud-monitor` how the cloud parses different field values. This lets you probe enum ranges and edge cases without having to reproduce every physical state on the appliance.

```
npx tsx tools/packet-sender.ts <device-uuid> <a> <s> <b5> <b6> <b7> [t1 v1] ...
```

## Add a test and submit a PR

- Add a unit test under [`rethink/tests/cloud/devices/`](https://github.com/anszom/rethink/tree/master/tests/cloud/devices), mirroring an existing one, that feeds captured packets through your class and asserts the decoded properties. Run the suite with `npm test`.
- Open a pull request — contributions are welcome!

# LLM-assisted workflow

Much of the manual workflow - watching traffic, correlating device packets with the cloud's interpretation, sweeping field values, drafting the device class - is mostly pattern-matching over packet captures, which an LLM coding agent can do well. The repository ships an [MCP](https://modelcontextprotocol.io) server, [`rethink/tools/mcp-server.ts`](https://github.com/anszom/rethink/blob/master/tools/mcp-server.ts), that exposes the reverse-engineering toolkit to such an agent. The [Guidelines](#guidelines) above apply just as much here - in particular, you must understand and be able to justify any generated code.

## Setup

The same [Setup](#setup) prerequisites apply: the device must be provisioned and bridged. For cloud observation, log in once with [`lgcloud-monitor.ts`](https://github.com/anszom/rethink/blob/master/tools/lgcloud-monitor.ts) (it prompts for your country code and a post-login URL, and stores the credentials in `oauth.json`).

Register the server with your agent. With Claude Code, a checked-in [`.mcp.json`](https://github.com/anszom/rethink/blob/master/.mcp.json) already declares it:

```json
{
  "mcpServers": {
    "rethink-agent": {
      "command": "npx",
      "args": ["tsx", "tools/mcp-server.ts"],
      "env": { "RETHINK_MGMT": "${RETHINK_MGMT:-localhost:44401}" }
    }
  }
}
```

Set the `RETHINK_MGMT` environment variable to your rethink-cloud management host[:port] if it isn't `localhost:44401` (or have the agent call `set_mgmt_host` at runtime).

## The toolkit

The server exposes the tools below. Everything that encodes or decodes packets uses the exact framing the device code uses, so CRC16 / AABB checksums / headers are handled for you.

- **`list_devices`** — enumerate connected devices, their model IDs, protocol, and bridge status. Usually the first call.
- **`set_mgmt_host`** — point the device tools at your management host for the session.
- **`decode_packet` / `encode_packet`** — turn framed hex into structured TLV/AABB fields and back. A decoded packet round-trips through `encode_packet` unchanged, so you can take a captured packet, change one field, and re-emit it.
- **`device_start` / `device_stop` / `read_device`** — live in-memory capture of a device's wire traffic (rx = fromDevice, tx = toDevice), decoded.
- **`cloud_start` / `cloud_stop` / `read_cloud`** — live capture of the real LG cloud's MQTT notification feed — the cloud's *decoded interpretation* of each state update. This is the ground-truth oracle (cf. [Cross-check](#cross-check-against-the-lg-clouds-own-parsing)) and works even for devices rethink can't translate yet.
- **`inject`** — send a packet. `fromDevice` fakes a device→cloud packet (safe); `toDevice` sends a command that **actuates the appliance** and is gated behind an explicit confirmation. Injection waits for rethink to echo the packet back, so an unknown id / dropped packet is a real error rather than a silent no-op.
- **`probe`** — the active-learning loop: sweep one field across a list of values, inject each as a fromDevice packet, and report how the cloud reacted to each — mapping enum ranges without physically reproducing every state. Each result carries `delivered` (did rethink accept it) and `observed` (the cloud's reaction), so "the cloud saw it but didn't react" is distinguishable from "never delivered".
- **`read_capture`** — read a JSONL capture file produced by [`rethink-capture.ts`](https://github.com/anszom/rethink/blob/master/tools/rethink-capture.ts), for offline analysis.

For a recorded, file-based session, [`rethink-capture.ts`](https://github.com/anszom/rethink/blob/master/tools/rethink-capture.ts) writes a device's wire traffic and the time-aligned cloud notifications to one JSONL timeline, with inline annotations you type as you operate the appliance:

```
npx tsx tools/rethink-capture.ts --cloud <mgmt-host[:port]> <device-uuid> capture.jsonl
```

## A typical session

1. `list_devices` to find the device UUID (and confirm it's bridged).
2. `device_start` and `cloud_start` to begin buffering both streams.
3. Operate the appliance and the LG app while the agent watches: it correlates each device packet (`read_device`) with the cloud notification it triggered (`read_cloud`) by timestamp, recovering field meanings — the self-supervised version of [mapping state updates](#map-state-updates-device--cloud) and [commands](#map-commands-cloud--device).
4. For ambiguous fields, the agent runs `probe` to sweep candidate values and read the cloud's interpretation directly.
5. The agent drafts the device class (using the [skeleton](#find-the-closest-existing-device-and-use-it-as-a-skeleton) guidance), builds test packets with `encode_packet`, and runs `npm test`, iterating until the decode round-trips.

Alternatively, instead of the live `device_start` / `cloud_start` buffers (steps 2–3), record a session to a file with [`rethink-capture.ts`](https://github.com/anszom/rethink/blob/master/tools/rethink-capture.ts) while you operate the appliance, then have the agent analyse it offline with `read_capture`. This decouples the hands-on data-gathering from the analysis — handy when the two happen at different times or on different machines — and yields a single annotated timeline you can re-read and share. The active-probing step (4) still needs a live connection.

The [finishing steps](#add-a-test-and-submit-a-pr) — a unit test and a pull request — are the same as the manual workflow.

## Caveats

- The cloud feed is the key enabler: it provides labelled ground truth, which turns field inference into a supervised problem — and it's the only generic decoder available for [AABB](AABBProtocol) devices.
- Correlation is a coarse time-window match, so a quiet account helps; concurrent activity on other devices can leak into the window.
- The agent *proposes*; you review. Entity modelling — sensible names, device classes, value ranges, which fields are even worth exposing — is a judgement call best left to a human, and is bound by the [Guidelines](#guidelines).
- Safety: only `toDevice` injection actuates hardware and it's gated; `probe` only injects `fromDevice`. Still, you are talking to a real appliance through the real cloud — supervise it.
