import HADevice from './base'
import { Device as Thinq2Device } from '../thinq2/device'
import { type Connection } from '../homeassistant'
import { type Metadata } from '../thinq'
import { allowExtendedType } from '@/util/casting'
import AABBDevice from './aabb_device'
import log from '@/util/logging'

/**
 * LG heat-pump dryer SoftAP model RH10V9_CH (deviceType 202).
 *
 * Reverse-engineering stub: the module stays MQTT-connected but may not push
 * UART status until a "start monitoring" command is sent (same pattern as many
 * washers: F0ED…). Without a handler, that poll never runs, so captures stay empty.
 *
 * This driver:
 *  - sends laundry-style monitor-enable on start (and periodically retries)
 *  - logs every inbound frame
 *  - publishes the last raw AABB body as a diagnostic sensor for RE
 *
 * Once we have real status frames, map fields like RV13* dryers and drop the probe.
 */
export default class Device extends AABBDevice {
    private monitorTimer: ReturnType<typeof setInterval> | undefined

    constructor(HA: Connection, thinq: Thinq2Device, meta: Metadata) {
        super(HA, thinq)

        const lastPacket = {
            platform: 'sensor',
            unique_id: '$deviceid-last_packet',
            state_topic: '$this/last_packet',
            name: 'Last packet (hex)',
            icon: 'mdi:hexadecimal',
            entity_category: 'diagnostic',
        }

        this.setConfig(
            allowExtendedType({
                ...HADevice.config(meta, { name: 'LG Dryer (probe)' }),
                components: {
                    last_packet: lastPacket,
                    packet_count: {
                        platform: 'sensor',
                        unique_id: '$deviceid-packet_count',
                        state_topic: '$this/packet_count',
                        name: 'Packet count',
                        icon: 'mdi:counter',
                        entity_category: 'diagnostic',
                        state_class: 'total_increasing',
                    },
                },
            }),
        )
    }

    private packetCount = 0

    start() {
        this.sendMonitorEnable()
        // Retry a few times — some modules only answer after MCU wakes.
        this.monitorTimer = setInterval(() => this.sendMonitorEnable(), 15_000)
        setTimeout(() => {
            if (this.monitorTimer) {
                clearInterval(this.monitorTimer)
                this.monitorTimer = undefined
            }
        }, 2 * 60_000)
    }

    drop() {
        if (this.monitorTimer) {
            clearInterval(this.monitorTimer)
            this.monitorTimer = undefined
        }
        super.drop()
    }

    /** Common ThinQ2 laundry "enable unsolicited status" inner body (AABB). */
    private sendMonitorEnable() {
        // Washer / WashTower style — works on many 20/30xx laundry platforms.
        const cmds = [
            'F0ED1121010000001800',
            // Fridge-family variant seen on some modules (harmless if ignored)
            'F0ED1211010000010400',
        ]
        for (const hex of cmds) {
            log('status', this.id, 'RH10V9 probe: sending monitor enable', hex)
            this.send(Buffer.from(hex, 'hex'))
        }
    }

    processData(buf: Buffer) {
        log('status', this.id, 'RH10V9 probe RX', buf.toString('hex'))
        this.packetCount++
        this.publishProperty('packet_count', this.packetCount)
        this.publishProperty('last_packet', buf.toString('hex'))
        super.processData(buf)
    }

    processAABB(buf: Buffer) {
        // Dump structure hints for RE without claiming field meanings yet.
        log(
            'status',
            this.id,
            `RH10V9 AABB len=${buf.length} type=0x${buf[0]?.toString(16)} sub=0x${buf[1]?.toString(16)}`,
        )
    }

    setProperty(_prop: string, _mqttValue: string) {
        // probe only
    }
}
