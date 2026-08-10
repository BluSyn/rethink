document.addEventListener('DOMContentLoaded', function () {
    M.Tooltip.init(document.querySelectorAll('.tooltipped'))
    M.Modal.init(document.querySelectorAll('.modal'))
    M.Autocomplete.init(document.querySelectorAll('.autocomplete'), {
        data: {
            '101 (Refrigerator)': null,
            '201 (Washer)': null,
            '202 (Dryer)': null,
            '204 (Dishwasher)': null,
            '223 (WashTower)': null,
            '301 (Gas Range)': null,
            '302 (Microwave)': null,
            '304 (Range Hood)': null,
            '401 (Air Conditioner)': null,
            '403 (Dehumidifier)': null,
            '404 (Humidifier)': null,
        },
    })
})

const STATUS_OK = `<i class="tiny material-icons" style="color:#3dd68c">check</i>`
const STATUS_ERROR = `<i class="tiny material-icons" style="color:#f07178">error</i>`
const STATUS_UNKNOWN = `<i class="tiny material-icons" style="color:#8b9bb0">question_mark</i>`

let ws
let deviceWs
let reconnectTimer
let deviceReconnectTimer
let bridge_status = false
let selectedDeviceId = null
/** @type {HTMLElement[]} ordered multi-selection (A, B, …) */
let selectedFrames = []
/** Anchor for shift+click range */
let frameSelectAnchor = null
/** Last decode result (for spans / hover) */
let lastDecode = null

const devices = {}
const baseUrl = new URL(window.location)
baseUrl.search = ''
baseUrl.hash = ''

get('status_rethink').innerHTML = STATUS_UNKNOWN
get('status_mqtt').innerHTML = STATUS_UNKNOWN
get('status_bridge').innerHTML = STATUS_UNKNOWN
get('status_bridge_text').innerText = 'Unknown'

function get(id) {
    return document.getElementById(id)
}

function escapeHtml(s) {
    return String(s)
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;')
}

function shortId(id) {
    if (!id || id.length <= 14) return id || ''
    return id.slice(0, 6) + '…' + id.slice(-4)
}

// ── Device table ──────────────────────────────────────────────────────────

class DeviceEntry {
    constructor(id, remoteState, parent) {
        this.id = id
        this.remoteState = remoteState
        this.row = document.createElement('tr')
        this.row.title = id
        parent.appendChild(this.row)
        this.updateDom()
    }

    destroy() {
        this.row.remove()
    }

    update(remoteState) {
        this.remoteState = remoteState
        this.updateDom()
    }

    updateDom() {
        const model = this.remoteState.model || '—'
        const platform = this.remoteState.platform || '—'
        const haChip = this.remoteState.mapped
            ? `<span class="chip chip-mapped">mapped</span>`
            : `<span class="chip chip-unmapped">raw</span>`

        this.row.innerHTML = `
            <td class="col-model">
                <div>${escapeHtml(model)}${
                    this.remoteState.mapped
                        ? ''
                        : ' <i class="material-icons tooltipped tiny" data-tooltip="Not mapped to HA" style="color:#f0b429;font-size:14px;vertical-align:middle">warning</i>'
                }</div>
                <div class="id-sub">${escapeHtml(shortId(this.id))}</div>
            </td>
            <td class="col-plat">${escapeHtml(platform)}</td>
            <td class="col-ha">${haChip}</td>
            <td class="col-bridge">
                <div class="switch" style="display:inline-block">
                    <label>Off <input type="checkbox"> <span class="lever"></span> On</label>
                </div>
                <div class="hide preloader-wrapper verysmall active" style="vertical-align:middle">
                    <div class="spinner-layer spinner-green-only">
                        <div class="circle-clipper left"><div class="circle"></div></div>
                        <div class="gap-patch"><div class="circle"></div></div>
                        <div class="circle-clipper right"><div class="circle"></div></div>
                    </div>
                </div>
            </td>`

        this.bridgeSwitch = this.row.querySelector('input[type=checkbox]')
        this.bridgeDiv = this.row.querySelector('.switch')
        this.spinner = this.row.querySelector('.preloader-wrapper')

        // Whole-row select (except bridge switch)
        this.row.onclick = (ev) => {
            if (ev.target.closest('.switch') || ev.target.closest('input') || ev.target.closest('.lever')) {
                return
            }
            selectDevice(this.id)
        }

        const startBridge = async (deviceType) => {
            this.bridgeBusy = true
            this.refreshUI()
            try {
                await fetchWrapper(`bridge/${this.id}/enable`, { deviceType }, { method: 'POST' })
                this.remoteState.bridged = true
            } finally {
                this.bridgeBusy = false
                this.refreshUI()
            }
        }
        const stopBridge = async () => {
            this.bridgeBusy = true
            this.refreshUI()
            try {
                await fetchWrapper(`bridge/${this.id}/disable`, {}, { method: 'POST' })
                this.remoteState.bridged = false
            } finally {
                this.bridgeBusy = false
                this.refreshUI()
            }
        }

        this.bridgeSwitch.onchange = (ev) => {
            ev.stopPropagation()
            if (this.bridgeSwitch.checked) {
                if (this.remoteState.deviceType) {
                    startBridge(this.remoteState.deviceType)
                } else {
                    get('btn_devicetype_continue').onclick = () => {
                        let devType = get('devtype-input').value
                        devType = devType.split(' ')[0]
                        startBridge(devType)
                        M.Modal.getInstance(get('devicetype_query')).close()
                    }
                    M.Modal.getInstance(get('devicetype_query')).open()
                }
            } else {
                stopBridge()
            }
        }
        this.bridgeSwitch.onclick = (ev) => ev.stopPropagation()

        Array.from(this.row.getElementsByClassName('tooltipped')).forEach((e) => M.Tooltip.init(e))
        this.refreshUI()
        this.row.classList.toggle('selected', selectedDeviceId === this.id)
    }

    refreshUI() {
        if (!this.bridgeSwitch) return
        if (this.bridgeBusy) {
            this.bridgeDiv.classList.add('hide')
            this.spinner.classList.remove('hide')
        } else {
            this.spinner.classList.add('hide')
            this.bridgeDiv.classList.remove('hide')
            this.bridgeSwitch.checked = !!this.remoteState.bridged
        }
        this.bridgeSwitch.disabled = !bridge_status
    }
}

function updateDevicesEmpty() {
    const empty = get('devices_empty')
    if (!empty) return
    if (Object.keys(devices).length === 0) empty.classList.remove('hide')
    else empty.classList.add('hide')
}

// ── Select device → monitor + decode ──────────────────────────────────────

function renderDetailBar(data, fallbackId) {
    const bar = get('detail_bar')
    if (!bar) return
    const fields = [
        ['ID', data.id || fallbackId || '—'],
        ['Model', data.modelId || data.model || '—'],
        ['Name', data.modelName || '—'],
        ['Platform', data.platform || '—'],
        ['Device type', data.deviceType || '—'],
        ['SW version', data.swVersion || '—'],
        ['HA mapped', data.mapped === true ? 'yes' : data.mapped === false ? 'no' : '—'],
        ['Bridged', data.bridged === true ? 'yes' : data.bridged === false ? 'no' : '—'],
        ['HA MQTT', data.haConnected === true ? 'connected' : data.haConnected === false ? 'disconnected' : '—'],
    ]
    bar.innerHTML = fields
        .map(
            ([k, v]) =>
                `<div class="di"><label>${escapeHtml(k)}</label><span title="${escapeHtml(
                    String(v),
                )}">${escapeHtml(String(v))}</span></div>`,
        )
        .join('')
}

async function selectDevice(id) {
    selectedDeviceId = id
    for (const d of Object.values(devices)) {
        d.row.classList.toggle('selected', d.id === id)
    }

    const wb = get('workbench')
    wb.classList.add('active')

    const local = devices[id]
    const model = (local && local.remoteState.model) || ''
    get('decode_model').value = model
    get('device_meta').textContent = `${model || '—'} · ${id}`
    get('device_status').textContent = 'connecting…'
    get('decode_source').textContent = ''
    renderDetailBar(
        {
            id,
            modelId: model,
            platform: local && local.remoteState.platform,
            deviceType: local && local.remoteState.deviceType,
            mapped: local && local.remoteState.mapped,
            bridged: local && local.remoteState.bridged,
        },
        id,
    )

    clearFrames()
    connectDeviceWs(id)

    // REST history (also arrives via WS history message)
    try {
        const res = await fetch(`${baseUrl}api/devices/${encodeURIComponent(id)}/frames`)
        const data = await res.json()
        if (data.ok && Array.isArray(data.frames) && data.frames.length) {
            // Only seed if WS history hasn't already filled the list
            if (get('messages').childElementCount === 0) {
                for (const f of data.frames) {
                    pushFrame(f.dir || 'rx', f.hex, f.injected, f.ts, true)
                }
            }
        }
    } catch (_) {
        /* ignore */
    }

    try {
        const res = await fetch(`${baseUrl}api/devices/${encodeURIComponent(id)}`)
        const data = await res.json()
        if (data.ok) {
            get('device_meta').textContent = `${data.modelId || model} · ${data.platform || ''} · ${
                data.mapped ? 'HA mapped' : 'unmapped'
            } · ${data.bridged ? 'bridged' : 'local'}`
            if (data.modelId) get('decode_model').value = data.modelId
            renderDetailBar(data, id)
        }
    } catch (_) {
        /* ignore */
    }
}

function deviceSocketUrl(id) {
    const url = new URL('device', baseUrl)
    url.protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    url.search = `?id=${encodeURIComponent(id)}`
    return url
}

function connectDeviceWs(id) {
    clearTimeout(deviceReconnectTimer)
    if (deviceWs) {
        deviceWs.onclose = deviceWs.onopen = deviceWs.onmessage = null
        try {
            deviceWs.close()
        } catch (_) {}
        deviceWs = null
    }

    let retry = 250
    const open = () => {
        if (selectedDeviceId !== id) return
        deviceWs = new WebSocket(deviceSocketUrl(id))
        deviceWs.onopen = () => {
            retry = 250
            get('device_status').textContent = 'waiting…'
        }
        deviceWs.onclose = () => {
            if (selectedDeviceId !== id) return
            get('device_status').textContent = 'reconnecting…'
            setInjectEnabled(false)
            deviceReconnectTimer = setTimeout(open, retry)
            retry = 5000
        }
        deviceWs.onmessage = (ev) => {
            if (selectedDeviceId !== id) return
            if (typeof ev.data !== 'string') return
            let json
            try {
                json = JSON.parse(ev.data)
            } catch {
                return
            }
            if (Array.isArray(json.history)) {
                // Prefer server history as baseline if list empty or only partial
                clearFrames()
                for (const f of json.history) {
                    const dir = f.rx != null ? 'rx' : 'tx'
                    const hex = f.rx != null ? f.rx : typeof f.tx === 'string' ? f.tx : JSON.stringify(f.tx)
                    pushFrame(dir, hex, f.injected, f.ts, true)
                }
            }
            if (json.rx != null) {
                const hex = typeof json.rx === 'string' ? json.rx : JSON.stringify(json.rx)
                pushFrame('rx', hex, json.injected, json.ts, false)
            }
            if (json.tx != null) {
                const hex = typeof json.tx === 'string' ? json.tx : JSON.stringify(json.tx)
                pushFrame('tx', hex, json.injected, json.ts, false)
            }
            if (json.status) {
                get('device_status').textContent = json.status
                setInjectEnabled(json.status === 'online')
            }
            if (json.meta && json.meta.modelId) {
                get('decode_model').value = json.meta.modelId
            }
        }
    }
    open()
}

function setInjectEnabled(on) {
    get('btn_send_to').disabled = !on
    get('btn_send_from').disabled = !on
}

function clearFrames() {
    get('messages').innerHTML = ''
    selectedFrames = []
    frameSelectAnchor = null
    updateDiffBanner()
    clearPayloadHighlight()
}

get('btn_clear_frames')?.addEventListener('click', clearFrames)

function formatTs(ts) {
    if (ts) {
        try {
            return new Date(Number(ts) || ts).toLocaleTimeString()
        } catch (_) {}
    }
    return new Date().toLocaleTimeString()
}

function parseTsMs(el) {
    const t = el && el.dataset.ts
    if (!t) return null
    const n = Number(t)
    return Number.isFinite(n) ? n : null
}

/** CLIP JSON from rethink handlers (setMaskingInfo, etc.) — not UART TLV hex. */
function isClipJsonPayload(s) {
    const t = String(s).trim()
    if (!t.startsWith('{')) return false
    try {
        const o = JSON.parse(t)
        return o && typeof o === 'object' && (o.cmd != null || o.type != null || o.data != null)
    } catch {
        return false
    }
}

function clipSummary(s) {
    try {
        const o = JSON.parse(s)
        const cmd = o.cmd || '?'
        const typ = o.type != null ? o.type : ''
        const data =
            o.data != null
                ? typeof o.data === 'string'
                    ? o.data
                    : JSON.stringify(o.data)
                : ''
        return `CLIP ${cmd}${typ !== '' ? ' type=' + typ : ''}${data ? ' ' + data : ''}`
    } catch {
        return s
    }
}

function pushFrame(dir, payload, injected, ts, fromHistory) {
    const messages = get('messages')
    const div = document.createElement('div')
    const raw = String(payload)
    const clip = isClipJsonPayload(raw)
    const tsMs = ts != null ? Number(ts) : Date.now()
    div.className = `frame ${dir}${clip ? ' clip' : ''}${injected ? ' injected' : ''}`
    div.dataset.payload = raw
    div.dataset.dir = dir
    div.dataset.kind = clip ? 'clip' : 'hex'
    div.dataset.ts = String(tsMs)
    const label = clip ? clipSummary(raw) : raw
    const show = label.length > 200 ? label.slice(0, 200) + '…' : label
    div.innerHTML = `<span class="ts">${escapeHtml(formatTs(tsMs))}</span><span class="dir">${
        clip ? 'clip' : dir
    }</span>${escapeHtml(show)}`
    div.addEventListener('click', (ev) => onFrameClick(ev, div))
    messages.appendChild(div)
    if (!fromHistory && get('autoscroll').checked) {
        messages.scrollTop = messages.scrollHeight
    }
    return div
}

function allFrameEls() {
    return Array.from(get('messages').querySelectorAll('.frame'))
}

function paintFrameSelection() {
    allFrameEls().forEach((el) => {
        el.classList.remove('selected', 'selected-a', 'selected-b')
    })
    selectedFrames.forEach((el, i) => {
        el.classList.add('selected')
        if (selectedFrames.length >= 2) {
            if (i === 0) el.classList.add('selected-a')
            else if (i === selectedFrames.length - 1) el.classList.add('selected-b')
        }
    })
    updateDiffBanner()
}

function framesInTimeOrder(els) {
    return [...els].sort((a, b) => {
        const ta = parseTsMs(a) ?? 0
        const tb = parseTsMs(b) ?? 0
        if (ta !== tb) return ta - tb
        // stable: DOM order as tie-breaker
        const all = allFrameEls()
        return all.indexOf(a) - all.indexOf(b)
    })
}

function updateDiffBanner() {
    const b = get('diff_banner')
    if (!b) return
    if (selectedFrames.length === 0) {
        b.textContent = ''
    } else if (selectedFrames.length === 1) {
        b.textContent = '1 frame · Ctrl/⌘+click multi · Shift+click range'
    } else {
        const ordered = framesInTimeOrder(selectedFrames)
        const ta = parseTsMs(ordered[0])
        const tb = parseTsMs(ordered[ordered.length - 1])
        const dt = ta != null && tb != null ? Math.abs(tb - ta) : null
        const rx = ordered.filter((e) => e.dataset.dir === 'rx').length
        const tx = ordered.filter((e) => e.dataset.dir === 'tx').length
        b.textContent = `${ordered.length} frames (rx=${rx} tx=${tx}) · span ${
            dt != null ? dt + ' ms' : '?'
        } · full sequence in text breakdown`
    }
}

function onFrameClick(ev, el) {
    ev.preventDefault()
    const multi = ev.ctrlKey || ev.metaKey
    const range = ev.shiftKey
    const frames = allFrameEls()

    if (range && frameSelectAnchor) {
        const i0 = frames.indexOf(frameSelectAnchor)
        const i1 = frames.indexOf(el)
        if (i0 >= 0 && i1 >= 0) {
            const lo = Math.min(i0, i1)
            const hi = Math.max(i0, i1)
            selectedFrames = frames.slice(lo, hi + 1)
        } else {
            selectedFrames = [el]
            frameSelectAnchor = el
        }
    } else if (multi) {
        const idx = selectedFrames.indexOf(el)
        if (idx >= 0) selectedFrames.splice(idx, 1)
        else selectedFrames.push(el)
        frameSelectAnchor = el
    } else {
        selectedFrames = [el]
        frameSelectAnchor = el
    }

    paintFrameSelection()

    if (selectedFrames.length >= 2) {
        runMultiFrameBreakdown(framesInTimeOrder(selectedFrames))
    } else if (selectedFrames.length === 1) {
        loadFrameIntoDecoder(selectedFrames[0])
    }
}

function loadFrameIntoDecoder(el) {
    const payload = el.dataset.payload || ''
    const dir = el.dataset.dir || 'rx'
    const kind = el.dataset.kind || 'hex'

    get('decode_hex').value = payload
    renderPayloadView(payload, null)
    get('decode_direction').value = dir === 'tx' ? 'toDevice' : 'fromDevice'
    get('decode_source').textContent =
        kind === 'clip' ? `CLIP JSON · ${dir}` : `${dir} · ${payload.length} hex chars`

    if (get('auto_decode').checked) {
        if (kind === 'clip') {
            renderClipBreakdown(payload)
        } else {
            runDecode()
        }
    }
}

function renderClipBreakdown(payload) {
    lastDecode = null
    let pretty = payload
    let cmd = '?'
    try {
        const body = JSON.parse(payload)
        pretty = JSON.stringify(body, null, 2)
        cmd = body.cmd || '?'
    } catch (_) {}

    get('tlv_body').innerHTML = `<tr><td colspan="4" class="empty-state">
        Not a UART/TLV frame — ThinQ2 <b>CLIP</b> command from rethink (device handler → cloud MQTT).
    </td></tr>`
    get('decode_summary').innerHTML =
        `kind=<b>CLIP</b> · cmd=<b>${escapeHtml(String(cmd))}</b> · skip TLV decode`
    get('text_breakdown').value =
        `# ThinQ CLIP command (not TLV)\n` +
        `Source: rethink device handler → MQTT CLIP (cmd/type/data)\n\n` +
        pretty
    renderPayloadView(payload, null)
}

// ── Payload hex view + tag hover highlight ────────────────────────────────

function clearPayloadHighlight() {
    const view = get('payload_view')
    if (!view) return
    view.querySelectorAll('.hex-byte.hl').forEach((n) => n.classList.remove('hl'))
}

function renderPayloadView(hexStr, highlightRange) {
    const view = get('payload_view')
    if (!view) return
    const hex = String(hexStr || '')
        .replace(/[^0-9a-fA-F]/g, '')
        .toLowerCase()
    if (!hex) {
        view.innerHTML = '<span class="empty-state">Select a frame…</span>'
        return
    }
    // Pair into bytes
    const parts = []
    for (let i = 0; i < hex.length; i += 2) {
        const byteIndex = i / 2
        const pair = hex.slice(i, i + 2)
        let cls = 'hex-byte'
        if (
            highlightRange &&
            byteIndex >= highlightRange[0] &&
            byteIndex < highlightRange[1]
        ) {
            cls += ' hl'
        }
        parts.push(`<span class="${cls}" data-bi="${byteIndex}">${pair}</span>`)
    }
    view.innerHTML = parts.join('')
}

function highlightPayloadBytes(byteStart, byteEnd) {
    const view = get('payload_view')
    if (!view) return
    view.querySelectorAll('.hex-byte').forEach((n) => {
        const bi = Number(n.dataset.bi)
        n.classList.toggle('hl', bi >= byteStart && bi < byteEnd)
    })
}

// ── Multi-frame delta ─────────────────────────────────────────────────────

async function decodePayloadSilent(payload, dir) {
    if (isClipJsonPayload(payload)) {
        return { kind: 'clip', payload, dir }
    }
    const direction = dir === 'tx' ? 'toDevice' : 'fromDevice'
    const model_id = get('decode_model').value || undefined
    const res = await fetch(`${baseUrl}api/re/export`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ hex: payload, direction, model_id }),
    })
    const data = await res.json()
    if (!data.ok) throw new Error(data.error || 'decode failed')
    return { kind: 'hex', dir, decode: data.decode, text: data.text }
}

function tagMapFromDecode(dec) {
    const map = new Map()
    const els = (dec && dec.elements) || []
    for (const e of els) {
        map.set(e.t, e)
    }
    return map
}

/** Device identity for RE paste context (from detail bar / selection). */
function deviceContextLines() {
    const id = selectedDeviceId || '—'
    const model = (get('decode_model') && get('decode_model').value) || '—'
    let platform = '—'
    let deviceType = '—'
    let mapped = '—'
    let bridged = '—'
    if (selectedDeviceId && devices[selectedDeviceId]) {
        const s = devices[selectedDeviceId].remoteState || {}
        platform = s.platform || platform
        deviceType = s.deviceType || deviceType
        mapped = s.mapped === true ? 'yes' : s.mapped === false ? 'no' : mapped
        bridged = s.bridged === true ? 'yes' : s.bridged === false ? 'no' : bridged
    }
    const bar = get('detail_bar')
    if (bar) {
        const grab = (label) => {
            const labs = bar.querySelectorAll('.di label')
            for (const lab of labs) {
                if (lab.textContent.trim().toLowerCase() === label.toLowerCase()) {
                    const span = lab.parentElement && lab.parentElement.querySelector('span')
                    return span ? span.textContent.trim() : null
                }
            }
            return null
        }
        platform = grab('Platform') || platform
        deviceType = grab('Device type') || deviceType
        mapped = grab('HA mapped') || mapped
        bridged = grab('Bridged') || bridged
        const name = grab('Name')
        const mid = grab('Model')
        return {
            id: grab('ID') || id,
            modelId: mid && mid !== '—' ? mid : model,
            modelName: name || '—',
            platform,
            deviceType,
            mapped,
            bridged,
        }
    }
    return { id, modelId: model, modelName: '—', platform, deviceType, mapped, bridged }
}

/**
 * Compact one-frame block for multi-frame transcripts (no nested full exports).
 * @param {number|string} index 1-based
 */
function formatFrameCompact(index, decoded, payload, dir, ts, t0) {
    const lines = []
    const kind = decoded.kind || (isClipJsonPayload(payload) ? 'clip' : 'hex')
    const iso = ts != null ? new Date(ts).toISOString() : '?'
    const rel = t0 != null && ts != null ? `+${ts - t0}ms` : ''
    const direction = dir === 'tx' ? 'toDevice' : dir === 'rx' ? 'fromDevice' : dir
    const hex =
        kind === 'hex' && decoded.decode && decoded.decode.hex
            ? decoded.decode.hex
            : String(payload || '')
                  .replace(/[^0-9a-fA-F]/g, '')
                  .toLowerCase() || payload

    const head = [`#${index}`, dir.toUpperCase(), direction, rel || iso].filter(Boolean).join(' ')
    lines.push(`### ${head}`)
    if (kind === 'hex' && decoded.decode) {
        const p = decoded.decode.protocol || '?'
        const crc = decoded.decode.crcOk != null ? ` crc=${decoded.decode.crcOk}` : ''
        const notes = (decoded.decode.notes || []).join('; ')
        lines.push(`protocol=${p}${crc}${notes ? ' · ' + notes : ''}`)
    } else if (kind === 'clip') {
        lines.push('protocol=CLIP')
    }
    lines.push(`hex: ${hex}`)

    if (kind === 'clip') {
        try {
            lines.push(JSON.stringify(JSON.parse(payload)))
        } catch {
            lines.push(payload)
        }
        lines.push('')
        return lines
    }

    if (kind === 'hex' && decoded.decode) {
        const dec = decoded.decode
        if (dec.protocol === 'UartBinary') {
            // Prefer compact server text if present, else body only
            if (decoded.text) {
                // strip leading # title lines already covered
                const body = decoded.text
                    .split('\n')
                    .filter((l) => !l.startsWith('# ') && !l.startsWith('modelId:') && !l.startsWith('direction:'))
                    .join('\n')
                    .trim()
                if (body) lines.push(body)
            } else if (dec.aabbBody) {
                lines.push(`body: ${dec.aabbBody}`)
            }
            lines.push('')
            return lines
        }
        const els = dec.elements || []
        if (!els.length) {
            if (dec.aabbBody) lines.push(`body: ${dec.aabbBody}`)
            else lines.push('tags: (none)')
        } else {
            const parts = els.map((e) => {
                const hx = '0x' + Number(e.t).toString(16).padStart(3, '0')
                if (e.known) return `${hx}:${e.name || '?'}=${e.v}`
                return `${hx}:UNKNOWN=${e.v}`
            })
            // Keep multi-line if many tags, single line if few
            if (parts.length <= 8) lines.push(`tags: ${parts.join(' ')}`)
            else {
                lines.push('tags:')
                for (const p of parts) lines.push(`  ${p}`)
            }
        }
        lines.push('')
        return lines
    }

    lines.push('(undecoded)')
    lines.push('')
    return lines
}

function tlvDeltaSummary(mapA, mapB) {
    const appeared = []
    const disappeared = []
    const changed = []
    const allTags = new Set([...mapA.keys(), ...mapB.keys()])
    for (const t of [...allTags].sort((x, y) => x - y)) {
        const ea = mapA.get(t)
        const eb = mapB.get(t)
        const name = (eb && eb.name) || (ea && ea.name) || null
        const label = `0x${t.toString(16)}${name ? '(' + name + ')' : ''}`
        if (!ea && eb) appeared.push(`${label}=${eb.v}${eb.known ? '' : '*'}`)
        else if (ea && !eb) disappeared.push(`${label} was ${ea.v}${ea.known ? '' : '*'}`)
        else if (ea && eb && ea.v !== eb.v)
            changed.push(`${label}: ${ea.v}→${eb.v}${ea.known && eb.known ? '' : '*'}`)
    }
    return { appeared, disappeared, changed }
}

/**
 * Multi-select breakdown: all frames in time order (rx+tx), compact; optional first↔last TLV delta.
 * @param {HTMLElement[]} ordered
 */
async function runMultiFrameBreakdown(ordered) {
    if (!ordered || ordered.length < 2) return
    const dev = deviceContextLines()
    const t0 = parseTsMs(ordered[0])
    const tLast = parseTsMs(ordered[ordered.length - 1])
    const span = t0 != null && tLast != null ? tLast - t0 : null

    const last = ordered[ordered.length - 1]
    get('decode_hex').value = last.dataset.payload || ''
    get('decode_direction').value = last.dataset.dir === 'tx' ? 'toDevice' : 'fromDevice'
    get('decode_source').textContent = `${ordered.length} frames · ${
        span != null ? span + 'ms' : '?'
    }`

    try {
        const decodedList = await Promise.all(
            ordered.map((el) =>
                decodePayloadSilent(el.dataset.payload || '', el.dataset.dir || 'rx'),
            ),
        )

        // UI focus: last frame
        const lastDec = decodedList[decodedList.length - 1]
        const lastPayload = last.dataset.payload || ''
        if (lastDec.kind === 'clip') {
            renderClipBreakdown(lastPayload)
        } else if (lastDec.decode) {
            renderDecode(lastDec.decode)
            lastDecode = lastDec.decode
            renderPayloadView(lastDec.decode.hex || lastPayload, null)
        }

        const lines = []
        lines.push('# ThinQ frame sequence')
        lines.push(
            `device: ${dev.modelId} type=${dev.deviceType} id=${dev.id} platform=${dev.platform} mapped=${dev.mapped} bridged=${dev.bridged}`,
        )
        lines.push(
            `frames: ${ordered.length} · t0=${
                t0 != null ? new Date(t0).toISOString() : '?'
            } · span_ms=${span != null ? span : '?'}`,
        )
        lines.push('')
        lines.push('## Sequence (time order)')

        ordered.forEach((el, i) => {
            const d = decodedList[i]
            const payload = el.dataset.payload || ''
            const dir = el.dataset.dir || 'rx'
            const ts = parseTsMs(el)
            lines.push(...formatFrameCompact(i + 1, d, payload, dir, ts, t0))
        })

        // Compact first↔last TLV delta when both are TLV-like
        const first = decodedList[0]
        const lastD = decodedList[decodedList.length - 1]
        if (
            first.kind === 'hex' &&
            lastD.kind === 'hex' &&
            first.decode &&
            lastD.decode &&
            first.decode.elements &&
            lastD.decode.elements &&
            first.decode.protocol !== 'UartBinary' &&
            lastD.decode.protocol !== 'UartBinary'
        ) {
            const mapA = tagMapFromDecode(first.decode)
            const mapB = tagMapFromDecode(lastD.decode)
            const { appeared, disappeared, changed } = tlvDeltaSummary(mapA, mapB)
            lines.push('## First→last TLV delta')
            lines.push(
                `protocols: ${first.decode.protocol} → ${lastD.decode.protocol} · * = unknown tag`,
            )
            if (!appeared.length && !disappeared.length && !changed.length) {
                lines.push('(no tag changes)')
            } else {
                if (appeared.length) lines.push(`+ ${appeared.join(' · ')}`)
                if (disappeared.length) lines.push(`- ${disappeared.join(' · ')}`)
                if (changed.length) lines.push(`~ ${changed.join(' · ')}`)
            }

            // Tag table annotations for last frame
            const body = get('tlv_body')
            if (body && lastD.decode.elements) {
                body.innerHTML = ''
                for (const el of lastD.decode.elements) {
                    const tr = document.createElement('tr')
                    const prev = mapA.get(el.t)
                    let delta = ''
                    if (!prev) delta = ' <span style="color:var(--ok)">(+)</span>'
                    else if (prev.v !== el.v)
                        delta = ` <span style="color:var(--warn)">(${prev.v}→${el.v})</span>`
                    const status = el.known ? 'known' : 'unknown'
                    const hasSpan = el.byteEnd != null && el.byteEnd > (el.byteStart || 0)
                    tr.className = hasSpan ? 'has-span' : ''
                    if (hasSpan) {
                        tr.dataset.byteStart = String(el.byteStart)
                        tr.dataset.byteEnd = String(el.byteEnd)
                    }
                    tr.innerHTML = `
                        <td class="${status}"><code>${escapeHtml(
                            el.hex || '0x' + Number(el.t).toString(16),
                        )}</code></td>
                        <td>${escapeHtml(el.name || '—')}${delta}</td>
                        <td><code>${escapeHtml(String(el.v))}</code></td>
                        <td class="${status}">${el.known ? 'known' : 'UNKNOWN'}</td>`
                    if (hasSpan) attachTagHover(tr)
                    body.appendChild(tr)
                }
            }
            get('decode_summary').innerHTML = `sequence ${ordered.length} · span <b>${
                span != null ? span + 'ms' : '?'
            }</b> · +${appeared.length} −${disappeared.length} ~${changed.length}`
        } else {
            get('decode_summary').innerHTML = `sequence ${ordered.length} · span <b>${
                span != null ? span + 'ms' : '?'
            }</b>`
        }

        get('text_breakdown').value = lines.join('\n').replace(/\n{3,}/g, '\n\n')
    } catch (err) {
        M.toast({ html: `sequence error: ${err}` })
    }
}

// Inject
get('btn_send_to').onclick = () => {
    if (!deviceWs || deviceWs.readyState !== WebSocket.OPEN) return
    let cmd = get('send_to').value.trim()
    if (!cmd) return
    if (cmd[0] === '{') {
        try {
            cmd = JSON.parse(cmd)
        } catch {
            M.toast({ html: 'invalid JSON' })
            return
        }
    }
    deviceWs.send(JSON.stringify({ sendToDevice: cmd }))
}
get('btn_send_from').onclick = () => {
    if (!deviceWs || deviceWs.readyState !== WebSocket.OPEN) return
    const hex = get('send_from').value.trim()
    if (!hex) return
    deviceWs.send(JSON.stringify({ sendFromDevice: hex }))
}

// ── Decode + text breakdown ───────────────────────────────────────────────

function attachTagHover(tr) {
    tr.addEventListener('mouseenter', () => {
        const b0 = tr.dataset.byteStart
        const b1 = tr.dataset.byteEnd
        if (b0 === '' || b1 === '' || b0 == null) return
        highlightPayloadBytes(Number(b0), Number(b1))
    })
    tr.addEventListener('mouseleave', () => {
        clearPayloadHighlight()
    })
}

function renderDecode(data) {
    lastDecode = data
    const body = get('tlv_body')
    body.innerHTML = ''
    const els = data.elements || []
    if (data.protocol === 'UartBinary') {
        const ba = data.binaryAnalysis || {}
        const env = ba.envelope || {}
        const hits = ba.heuristics || []
        body.innerHTML = `<tr><td colspan="4" class="empty-state">
            <b>UartBinary</b> — not climate TLV.
            kind=0x${Number(env.kind || 0).toString(16)} b5=0x${Number(env.byte5 || 0).toString(16)}
            b6=0x${Number(env.byte6 || 0).toString(16)} body=${ba.body_len || '?'}B ·
            ${hits.length} heuristic candidate(s) — see text breakdown.
        </td></tr>`
        get('decode_summary').innerHTML = `protocol=<b>UartBinary</b> · attempting structured RE (envelope + heuristics)`
        renderPayloadView(data.hex || get('decode_hex').value, null)
        return
    }
    if (els.length === 0) {
        body.innerHTML = `<tr><td colspan="4" class="empty-state">No TLV elements (protocol=${escapeHtml(
            data.protocol || '?',
        )}${data.aabbBody ? '; AABB body present — see text breakdown' : ''})</td></tr>`
    } else {
        for (const el of els) {
            const tr = document.createElement('tr')
            const status = el.known ? 'known' : 'unknown'
            const hasSpan = el.byteEnd != null && el.byteEnd > (el.byteStart || 0)
            tr.className = hasSpan ? 'has-span' : ''
            if (hasSpan) {
                tr.dataset.byteStart = String(el.byteStart)
                tr.dataset.byteEnd = String(el.byteEnd)
            }
            tr.innerHTML = `
                <td class="${status}"><code>${escapeHtml(el.hex || '0x' + Number(el.t).toString(16))}</code></td>
                <td>${escapeHtml(el.name || '—')}</td>
                <td><code>${escapeHtml(String(el.v))}</code></td>
                <td class="${status}">${el.known ? 'known' : 'UNKNOWN'}</td>`
            if (hasSpan) attachTagHover(tr)
            body.appendChild(tr)
        }
    }
    const sum = get('decode_summary')
    const unk = data.unknownCount ?? 0
    sum.innerHTML = `protocol=<b>${escapeHtml(data.protocol || '?')}</b> · dir=${escapeHtml(
        data.direction || '?',
    )} · unknowns=<b style="color:${unk ? 'var(--unknown)' : 'var(--ok)'}">${unk}</b>${
        data.crcOk == null ? '' : ' · crcOk=' + data.crcOk
    }${(data.notes || []).length ? ' · ' + escapeHtml(data.notes.join('; ')) : ''}`

    renderPayloadView(data.hex || get('decode_hex').value, null)
}

/** Decode TLV/AABB and always refresh the text breakdown. */
async function runDecode() {
    const hex = get('decode_hex').value.trim()
    if (!hex) {
        M.toast({ html: 'Nothing to decode' })
        return
    }
    if (isClipJsonPayload(hex)) {
        renderClipBreakdown(hex)
        return
    }
    const direction = get('decode_direction').value
    const model_id = get('decode_model').value || undefined
    try {
        const res = await fetch(`${baseUrl}api/re/export`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ hex, direction, model_id }),
        })
        const data = await res.json()
        if (!data.ok) {
            M.toast({ html: data.error || 'decode failed' })
            return
        }
        if (data.decode) renderDecode(data.decode)
        get('text_breakdown').value = data.text || ''
    } catch (err) {
        M.toast({ html: `decode error: ${err}` })
    }
}

// Keep payload view in sync when user pastes hex manually
get('decode_hex')?.addEventListener('input', () => {
    renderPayloadView(get('decode_hex').value, null)
})

async function copyExport() {
    const text = get('text_breakdown').value
    if (!text) {
        M.toast({ html: 'Nothing to copy yet' })
        return
    }
    try {
        await navigator.clipboard.writeText(text)
        M.toast({ html: 'Copied text breakdown' })
    } catch {
        get('text_breakdown').select()
        document.execCommand('copy')
        M.toast({ html: 'Copied (fallback)' })
    }
}

get('btn_decode')?.addEventListener('click', runDecode)
get('btn_copy_export')?.addEventListener('click', copyExport)

// ── Status WebSocket ──────────────────────────────────────────────────────

let retryDelay = 250

function connect() {
    clearTimeout(reconnectTimer)
    if (ws) {
        ws.onclose = ws.onopen = ws.onmessage = null
        try {
            ws.close()
        } catch (_) {}
    }
    ws = new WebSocket(baseUrl + 'ws')

    ws.onclose = () => {
        get('status_rethink').innerHTML = STATUS_ERROR
        get('status_mqtt').innerHTML = STATUS_UNKNOWN
        document.body.classList.add('offline')
        reconnectTimer = setTimeout(connect, retryDelay)
        retryDelay = 5000
    }

    ws.onopen = () => {
        retryDelay = 250
        get('status_rethink').innerHTML = STATUS_OK
        document.body.classList.remove('offline')
    }

    ws.onmessage = (ev) => {
        if (typeof ev.data !== 'string') return
        const json = JSON.parse(ev.data)
        if (typeof json.ha === 'boolean') {
            get('status_mqtt').innerHTML = json.ha ? STATUS_OK : STATUS_ERROR
        }

        if (typeof json.devices === 'object') {
            Object.keys(devices)
                .filter((id) => !json.devices[id])
                .forEach((id) => {
                    devices[id].destroy()
                    delete devices[id]
                    if (selectedDeviceId === id) {
                        selectedDeviceId = null
                        get('workbench').classList.remove('active')
                        if (deviceWs) {
                            try {
                                deviceWs.close()
                            } catch (_) {}
                        }
                    }
                })
            for (const id in json.devices) {
                const j = json.devices[id]
                if (!devices[id]) devices[id] = new DeviceEntry(id, j, get('devices_body'))
                else devices[id].update(j)
            }
            updateDevicesEmpty()
            // Deep-link or re-select after list refresh
            if (selectedDeviceId && devices[selectedDeviceId] && !get('workbench').classList.contains('active')) {
                selectDevice(selectedDeviceId)
            }
        }

        if (typeof json.bridge === 'object') {
            bridge_status = json.bridge.loggedIn
            if (json.bridge.loggedIn === true) {
                get('btn_thinq_login').classList.add('hide')
                get('btn_thinq_logout').classList.remove('hide')
                get('status_bridge').innerHTML = STATUS_OK
                get('status_bridge_text').innerText = 'Ok'
            } else {
                get('btn_thinq_login').classList.remove('hide')
                get('btn_thinq_logout').classList.add('hide')
                get('status_bridge').innerHTML = STATUS_ERROR
                get('status_bridge_text').innerText = 'Not configured'
            }
            for (const id in devices) devices[id].refreshUI()
        }

        if (typeof json.status === 'string') {
            M.toast({ html: json.status })
        }
    }
}

get('btn_thinq_login_continue').onclick = () => {
    if (!get('country_code').validity.valid) return
    const countryCode = get('country_code').value.toUpperCase()
    window.open(`${baseUrl}thinq_login?countryCode=${countryCode}`, '_blank')
}

get('btn_thinq_login_complete').onclick = async () => {
    if (!get('country_code').validity.valid) return
    if (!get('login_url').validity.valid) return
    const countryCode = get('country_code').value.toUpperCase()
    const url = get('login_url').value
    await fetchWrapper(`thinq_login_accept`, { url, countryCode }, { method: 'POST' })
    M.Modal.getInstance(get('thinq_login')).close()
}

get('btn_thinq_logout_continue').onclick = async () => {
    await fetchWrapper(`thinq_logout`, {}, { method: 'POST' })
    M.Modal.getInstance(get('thinq_logout')).close()
}

window.addEventListener('pageshow', (ev) => {
    if (ev.persisted) connect()
})

async function fetchWrapper(path, body, options) {
    if (options.method !== 'GET') {
        if (!options.headers) options.headers = {}
        options.headers['Content-type'] = 'application/json'
    }
    options.body = JSON.stringify(body)
    try {
        const response = await fetch(`${baseUrl}${path}`, options)
        if (response.status >= 300) M.toast({ html: `HTTP error ${response.status}: ${await response.text()}` })
        return response
    } catch (err) {
        M.toast({ html: `FETCH error: ${err}` })
    }
}

// Deep-link: ?id=DEVICE still works (select on connect when device appears)
const bootId = new URLSearchParams(window.location.search).get('id')
if (bootId) {
    // Will select once device list arrives; also open workbench early
    selectedDeviceId = bootId
}

updateDevicesEmpty()
connect()

// After devices appear, apply boot selection
const _origOnMsg = null
// Poll once shortly after load for deep-link
setTimeout(() => {
    if (bootId && devices[bootId]) selectDevice(bootId)
}, 800)
