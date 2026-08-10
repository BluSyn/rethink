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

function updateDiffBanner() {
    const b = get('diff_banner')
    if (!b) return
    if (selectedFrames.length === 0) {
        b.textContent = ''
    } else if (selectedFrames.length === 1) {
        b.textContent = '1 frame selected · Ctrl/⌘+click another for delta · Shift+click for range'
    } else {
        const a = selectedFrames[0]
        const z = selectedFrames[selectedFrames.length - 1]
        const ta = parseTsMs(a)
        const tb = parseTsMs(z)
        const dt = ta != null && tb != null ? Math.abs(tb - ta) : null
        b.textContent = `${selectedFrames.length} frames · A↔B Δt=${
            dt != null ? dt + ' ms' : '?'
        } · showing delta of first & last`
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
        runFrameDelta(selectedFrames[0], selectedFrames[selectedFrames.length - 1])
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
 * Single-frame section (raw hex + TLV list + full export) for paste/debug context.
 * @param {'A'|'B'} label
 */
function formatFrameSection(label, decoded, payload, dir, ts) {
    const lines = []
    const kind = decoded.kind || (isClipJsonPayload(payload) ? 'clip' : 'hex')
    const iso = ts != null ? new Date(ts).toISOString() : '?'
    const direction = dir === 'tx' ? 'toDevice' : 'fromDevice'
    const hex =
        kind === 'hex' && decoded.decode && decoded.decode.hex
            ? decoded.decode.hex
            : String(payload || '')
                  .replace(/[^0-9a-fA-F]/g, '')
                  .toLowerCase() || payload

    lines.push(`## Frame ${label}`)
    lines.push(`label: ${label}`)
    lines.push(`timestamp: ${iso}`)
    lines.push(`direction: ${direction} (${dir})`)
    lines.push(`kind: ${kind}`)
    if (kind === 'hex' && decoded.decode) {
        lines.push(`protocol: ${decoded.decode.protocol || '?'}`)
        if (decoded.decode.crcOk != null) lines.push(`crcOk: ${decoded.decode.crcOk}`)
        if ((decoded.decode.notes || []).length)
            lines.push(`notes: ${decoded.decode.notes.join('; ')}`)
    }
    lines.push(`hex: ${hex}`)
    lines.push('')

    if (kind === 'clip') {
        lines.push('### CLIP payload')
        try {
            lines.push(JSON.stringify(JSON.parse(payload), null, 2))
        } catch {
            lines.push(payload)
        }
        lines.push('')
        return lines
    }

    if (kind === 'hex' && decoded.decode) {
        const els = decoded.decode.elements || []
        lines.push('### TLV elements')
        if (!els.length) {
            lines.push('(none)')
            if (decoded.decode.aabbBody) lines.push(`aabbBody: ${decoded.decode.aabbBody}`)
        } else {
            for (const e of els) {
                const hx = '0x' + Number(e.t).toString(16).padStart(3, '0')
                if (e.known) lines.push(`- ${hx} (${e.name || '?'}) = ${e.v}`)
                else lines.push(`- ${hx} **UNKNOWN** = ${e.v}`)
            }
        }
        lines.push('')
        if (decoded.text) {
            lines.push('### Full single-frame export')
            lines.push(decoded.text.trim())
            lines.push('')
        }
        return lines
    }

    lines.push('(unable to decode frame)')
    lines.push('')
    return lines
}

async function runFrameDelta(elA, elB) {
    const payloadA = elA.dataset.payload || ''
    const payloadB = elB.dataset.payload || ''
    const dirA = elA.dataset.dir || 'rx'
    const dirB = elB.dataset.dir || 'rx'
    const tsA = parseTsMs(elA)
    const tsB = parseTsMs(elB)
    const dt = tsA != null && tsB != null ? Math.abs(tsB - tsA) : null
    const dev = deviceContextLines()

    get('decode_hex').value = payloadB
    get('decode_direction').value = dirB === 'tx' ? 'toDevice' : 'fromDevice'
    get('decode_source').textContent = `Δ A→B · ${dt != null ? dt + ' ms' : 'Δt ?'}`

    try {
        const [a, b] = await Promise.all([
            decodePayloadSilent(payloadA, dirA),
            decodePayloadSilent(payloadB, dirB),
        ])

        // Show B in the table/view as the "current" frame
        if (b.kind === 'clip') {
            renderClipBreakdown(payloadB)
        } else if (b.decode) {
            renderDecode(b.decode)
            lastDecode = b.decode
            renderPayloadView(b.decode.hex || payloadB, null)
        }

        const lines = []
        lines.push('# Frame delta (rethink management)')
        lines.push('')
        lines.push('## Device')
        lines.push(`modelId: ${dev.modelId}`)
        lines.push(`modelName: ${dev.modelName}`)
        lines.push(`deviceId: ${dev.id}`)
        lines.push(`platform: ${dev.platform}`)
        lines.push(`deviceType: ${dev.deviceType}`)
        lines.push(`haMapped: ${dev.mapped}`)
        lines.push(`bridged: ${dev.bridged}`)
        lines.push('')
        lines.push('## Timing')
        lines.push(`time_delta_ms: ${dt != null ? dt : 'unknown'}`)
        lines.push(
            `A: ts=${tsA != null ? new Date(tsA).toISOString() : '?'} dir=${dirA} kind=${a.kind}`,
        )
        lines.push(
            `B: ts=${tsB != null ? new Date(tsB).toISOString() : '?'} dir=${dirB} kind=${b.kind}`,
        )
        lines.push('')

        // Full context for each frame (raw + decode), same spirit as single-frame breakdown
        lines.push(...formatFrameSection('A', a, payloadA, dirA, tsA))
        lines.push(...formatFrameSection('B', b, payloadB, dirB, tsB))

        if (a.kind === 'clip' || b.kind === 'clip') {
            lines.push('## Note')
            lines.push('One or both frames are CLIP JSON (rethink→device control), not TLV.')
            lines.push('Tag-level delta is skipped when CLIP is involved; see Frame A/B sections above.')
            get('text_breakdown').value = lines.join('\n')
            get('decode_summary').innerHTML = `delta · CLIP involved · Δt=<b>${
                dt != null ? dt + 'ms' : '?'
            }</b>`
            return
        }

        const mapA = tagMapFromDecode(a.decode)
        const mapB = tagMapFromDecode(b.decode)
        const allTags = new Set([...mapA.keys(), ...mapB.keys()])
        const appeared = []
        const disappeared = []
        const changed = []
        const same = []
        const unkChanged = []

        for (const t of [...allTags].sort((x, y) => x - y)) {
            const ea = mapA.get(t)
            const eb = mapB.get(t)
            const name = (eb && eb.name) || (ea && ea.name) || null
            const label = `0x${t.toString(16)} (${name || '?'})`
            if (!ea && eb) {
                appeared.push({ t, eb, label })
                if (!eb.known) unkChanged.push(`+ ${label} = ${eb.v}`)
            } else if (ea && !eb) {
                disappeared.push({ t, ea, label })
                if (!ea.known) unkChanged.push(`- ${label} was ${ea.v}`)
            } else if (ea && eb && ea.v !== eb.v) {
                changed.push({ t, ea, eb, label })
                if (!eb.known || !ea.known) unkChanged.push(`~ ${label}: ${ea.v} → ${eb.v}`)
            } else if (ea && eb) {
                same.push({ t, ea, label })
            }
        }

        lines.push('## Delta summary')
        lines.push(`protocol A=${a.decode.protocol} B=${b.decode.protocol}`)
        lines.push('')
        lines.push('### Appeared in B (not in A)')
        if (!appeared.length) lines.push('(none)')
        else
            appeared.forEach(({ label, eb }) =>
                lines.push(`- ${label} = ${eb.v} ${eb.known ? '[known]' : '**UNKNOWN**'}`),
            )
        lines.push('')
        lines.push('### Disappeared (in A, not B)')
        if (!disappeared.length) lines.push('(none)')
        else
            disappeared.forEach(({ label, ea }) =>
                lines.push(`- ${label} was ${ea.v} ${ea.known ? '[known]' : '**UNKNOWN**'}`),
            )
        lines.push('')
        lines.push('### Changed values')
        if (!changed.length) lines.push('(none)')
        else
            changed.forEach(({ label, ea, eb }) =>
                lines.push(
                    `- ${label}: ${ea.v} → ${eb.v} ${
                        eb.known && ea.known ? '[known]' : '**UNKNOWN involved**'
                    }`,
                ),
            )
        lines.push('')
        lines.push('### Unchanged tags')
        lines.push(`(${same.length} tags)`)
        lines.push('')
        lines.push('### Unknown-tag focus (appeared / disappeared / changed)')
        if (!unkChanged.length) lines.push('(none — all delta tags are catalogued)')
        else unkChanged.forEach((s) => lines.push(s))
        lines.push('')
        lines.push('### Hint for RE')
        lines.push(
            `If Δt is small and a single unknown tag changed, it likely encodes the action between the two samples.`,
        )
        lines.push(
            `Prefer comparing frames of the same UART kind/protocol (e.g. both climate values a70204…).`,
        )

        get('text_breakdown').value = lines.join('\n')
        get('decode_summary').innerHTML = `delta A→B · Δt=<b>${
            dt != null ? dt + 'ms' : '?'
        }</b> · +${appeared.length} −${disappeared.length} ~${changed.length} · unkΔ=${
            unkChanged.length
        }`

        // Tag table shows B's tags, with delta annotation in name column via data attrs
        const body = get('tlv_body')
        body.innerHTML = ''
        const els = (b.decode && b.decode.elements) || []
        for (const el of els) {
            const tr = document.createElement('tr')
            const prev = mapA.get(el.t)
            let delta = ''
            if (!prev) delta = ' <span style="color:var(--ok)">(+new)</span>'
            else if (prev.v !== el.v)
                delta = ` <span style="color:var(--warn)">(${prev.v}→${el.v})</span>`
            const status = el.known ? 'known' : 'unknown'
            tr.className = el.hexStart != null || el.byteStart != null ? 'has-span' : ''
            tr.dataset.byteStart = el.byteStart != null ? el.byteStart : ''
            tr.dataset.byteEnd = el.byteEnd != null ? el.byteEnd : ''
            tr.innerHTML = `
                <td class="${status}"><code>${escapeHtml(el.hex || '0x' + Number(el.t).toString(16))}</code></td>
                <td>${escapeHtml(el.name || '—')}${delta}</td>
                <td><code>${escapeHtml(String(el.v))}</code></td>
                <td class="${status}">${el.known ? 'known' : 'UNKNOWN'}</td>`
            attachTagHover(tr)
            body.appendChild(tr)
        }
    } catch (err) {
        M.toast({ html: `delta error: ${err}` })
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
