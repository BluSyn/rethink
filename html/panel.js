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
let selectedFrameEl = null

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
    selectedFrameEl = null
}

get('btn_clear_frames')?.addEventListener('click', clearFrames)

function formatTs(ts) {
    if (ts) {
        try {
            return new Date(ts).toLocaleTimeString()
        } catch (_) {}
    }
    return new Date().toLocaleTimeString()
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
    div.className = `frame ${dir}${clip ? ' clip' : ''}${injected ? ' injected' : ''}`
    div.dataset.payload = raw
    div.dataset.dir = dir
    div.dataset.kind = clip ? 'clip' : 'hex'
    const label = clip ? clipSummary(raw) : raw
    const show = label.length > 200 ? label.slice(0, 200) + '…' : label
    div.innerHTML = `<span class="ts">${escapeHtml(formatTs(ts))}</span><span class="dir">${
        clip ? 'clip' : dir
    }</span>${escapeHtml(show)}`
    div.onclick = () => loadFrameIntoDecoder(div)
    messages.appendChild(div)
    if (!fromHistory && get('autoscroll').checked) {
        messages.scrollTop = messages.scrollHeight
    }
    return div
}

function loadFrameIntoDecoder(el) {
    if (selectedFrameEl) selectedFrameEl.classList.remove('selected')
    selectedFrameEl = el
    el.classList.add('selected')

    const payload = el.dataset.payload || ''
    const dir = el.dataset.dir || 'rx'
    const kind = el.dataset.kind || 'hex'

    get('decode_hex').value = payload
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
    let pretty = payload
    let cmd = '?'
    let body = null
    try {
        body = JSON.parse(payload)
        pretty = JSON.stringify(body, null, 2)
        cmd = body.cmd || '?'
    } catch (_) {}

    get('tlv_body').innerHTML = `<tr><td colspan="4" class="empty-state">
        Not a UART/TLV frame — ThinQ2 <b>CLIP</b> command from rethink (device handler → cloud MQTT),
        e.g. setMaskingInfo after values arrive. Not Home Assistant.
    </td></tr>`
    get('decode_summary').innerHTML =
        `kind=<b>CLIP</b> · cmd=<b>${escapeHtml(String(cmd))}</b> · skip TLV decode`
    get('text_breakdown').value =
        `# ThinQ CLIP command (not TLV)\n` +
        `Source: rethink device handler → MQTT CLIP (cmd/type/data)\n` +
        `Not from Home Assistant discovery; not an AABB/TLV wire frame.\n\n` +
        pretty
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

function renderDecode(data) {
    const body = get('tlv_body')
    body.innerHTML = ''
    const els = data.elements || []
    if (els.length === 0) {
        body.innerHTML = `<tr><td colspan="4" class="empty-state">No TLV elements (protocol=${escapeHtml(
            data.protocol || '?',
        )}${data.aabbBody ? '; AABB body present — see text breakdown' : ''})</td></tr>`
    } else {
        for (const el of els) {
            const tr = document.createElement('tr')
            const status = el.known ? 'known' : 'unknown'
            tr.innerHTML = `
                <td class="${status}"><code>${escapeHtml(el.hex || '0x' + Number(el.t).toString(16))}</code></td>
                <td>${escapeHtml(el.name || '—')}</td>
                <td><code>${escapeHtml(String(el.v))}</code></td>
                <td class="${status}">${el.known ? 'known' : 'UNKNOWN'}</td>`
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
}

/** Decode TLV/AABB and always refresh the text breakdown (was "LLM export"). */
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
        // Prefer /api/re/export — includes decode + full text breakdown in one call
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
