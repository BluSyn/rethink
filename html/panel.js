document.addEventListener('DOMContentLoaded', function () {
    M.Tooltip.init(document.querySelectorAll('.tooltipped'))
    M.Modal.init(document.querySelectorAll('.modal'))
    M.FormSelect.init(document.querySelectorAll('select'))
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

let ws
let reconnectTimer
const STATUS_OK = `<i class="tiny material-icons" style="color:#3dd68c">check</i>`
const STATUS_ERROR = `<i class="tiny material-icons" style="color:#f07178">error</i>`
const STATUS_UNKNOWN = `<i class="tiny material-icons" style="color:#8b9bb0">question_mark</i>`
let bridge_status = false
let selectedDeviceId = null

get('status_rethink').innerHTML = STATUS_UNKNOWN
get('status_mqtt').innerHTML = STATUS_UNKNOWN
get('status_bridge').innerHTML = STATUS_UNKNOWN
get('status_bridge_text').innerText = 'Unknown'

const devices = {}

const baseUrl = new URL(window.location)
baseUrl.search = ''
baseUrl.hash = ''

class DeviceEntry {
    constructor(id, remoteState, parent) {
        this.id = id
        this.remoteState = remoteState
        this.row = document.createElement('tr')
        this.updateDom()
        parent.appendChild(this.row)
    }

    destroy() {
        this.row.remove()
    }

    update(remoteState) {
        this.remoteState = remoteState
        this.updateDom()
    }

    updateDom() {
        const children = []

        let td = document.createElement('td')
        td.innerHTML = `<code style="font-size:0.8rem">${escapeHtml(this.id)}</code>`
        children.push(td)

        td = document.createElement('td')
        let model = escapeHtml(this.remoteState.model || '')
        if (!this.remoteState.mapped) {
            model += ` <i class="material-icons tooltipped tiny" data-position="bottom" data-tooltip="Not mapped to Home Assistant (unsupported modelId)" style="color:#f0b429">warning</i>`
        }
        td.innerHTML = model
        children.push(td)

        td = document.createElement('td')
        td.innerText = this.remoteState.platform || ''
        children.push(td)

        td = document.createElement('td')
        td.innerHTML = this.remoteState.mapped
            ? `<span class="chip chip-mapped">mapped</span>`
            : `<span class="chip chip-unmapped">raw</span>`
        children.push(td)

        td = document.createElement('td')
        td.style = 'width: 9em'
        td.innerHTML = `
            <div class="switch">
                <label>Off <input type="checkbox"> <span class="lever"></span>On</label>
            </div>
            <div class="hide preloader-wrapper verysmall active">
                <div class="spinner-layer spinner-green-only">
                <div class="circle-clipper left"><div class="circle"></div></div>
                <div class="gap-patch"><div class="circle"></div></div>
                <div class="circle-clipper right"><div class="circle"></div></div>
                </div>
            </div>`
        children.push(td)

        this.bridgeSwitch = td.getElementsByTagName('input')[0]
        this.bridgeDiv = td.getElementsByClassName('switch')[0]
        this.spinner = td.getElementsByClassName('preloader-wrapper')[0]

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

        this.bridgeSwitch.onchange = () => {
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

        td = document.createElement('td')
        td.style = 'white-space:nowrap'
        td.innerHTML = `
            <a class="btn-flat waves-effect white-text tooltipped" data-tooltip="Detail" data-action="detail"><i class="material-icons">info</i></a>
            <a class="btn-flat waves-effect white-text tooltipped" data-tooltip="Monitor" href="monitor?id=${encodeURIComponent(this.id)}"><i class="material-icons">troubleshoot</i></a>`
        td.querySelector('[data-action="detail"]').onclick = (ev) => {
            ev.preventDefault()
            selectDevice(this.id)
        }
        children.push(td)

        this.row.replaceChildren(...children)
        Array.from(this.row.getElementsByClassName('tooltipped')).forEach((e) => M.Tooltip.init(e))
        this.refreshUI()

        if (selectedDeviceId === this.id) {
            this.row.style.background = 'rgba(61, 156, 240, 0.12)'
        } else {
            this.row.style.background = ''
        }
    }

    refreshUI() {
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

function escapeHtml(s) {
    return String(s)
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;')
}

function updateDevicesEmpty() {
    const empty = get('devices_empty')
    if (!empty) return
    if (Object.keys(devices).length === 0) empty.classList.remove('hide')
    else empty.classList.add('hide')
}

async function selectDevice(id) {
    selectedDeviceId = id
    for (const d of Object.values(devices)) d.updateDom()
    const placeholder = get('detail_placeholder')
    const content = get('detail_content')
    placeholder.classList.add('hide')
    content.classList.remove('hide')
    get('detail_monitor_link').href = `monitor?id=${encodeURIComponent(id)}`

    // Prefill decode modelId
    const local = devices[id]
    if (local && local.remoteState.model) {
        get('decode_model').value = local.remoteState.model
    }

    try {
        const res = await fetch(`${baseUrl}api/devices/${encodeURIComponent(id)}`)
        const data = await res.json()
        if (!data.ok) {
            M.toast({ html: data.error || 'detail failed' })
            return
        }
        const grid = get('detail_grid')
        const fields = [
            ['ID', data.id],
            ['Model', data.modelId],
            ['Name', data.modelName],
            ['Platform', data.platform],
            ['Device type', data.deviceType || '—'],
            ['SW version', data.swVersion || '—'],
            ['HA mapped', data.mapped ? 'yes' : 'no'],
            ['Bridged', data.bridged ? 'yes' : 'no'],
            ['HA MQTT', data.haConnected ? 'connected' : 'disconnected'],
        ]
        grid.innerHTML = fields
            .map(
                ([k, v]) =>
                    `<div class="detail-item"><label>${escapeHtml(k)}</label><span>${escapeHtml(
                        v == null ? '—' : String(v),
                    )}</span></div>`,
            )
            .join('')
    } catch (err) {
        M.toast({ html: `detail error: ${err}` })
    }
}

get('detail_refresh')?.addEventListener('click', () => {
    if (selectedDeviceId) selectDevice(selectedDeviceId)
})

// ── RE decode / LLM export ────────────────────────────────────────────────

function renderDecode(data) {
    const body = get('tlv_body')
    body.innerHTML = ''
    const els = data.elements || []
    if (els.length === 0) {
        body.innerHTML = `<tr><td colspan="4" class="empty-state">No TLV elements (protocol=${escapeHtml(
            data.protocol || '?',
        )}${data.aabbBody ? '; AABB body present' : ''})</td></tr>`
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
    sum.textContent = `protocol=${data.protocol || '?'} · direction=${data.direction || '?'} · unknowns=${
        data.unknownCount ?? 0
    }${data.crcOk == null ? '' : ' · crcOk=' + data.crcOk}${(data.notes || []).length ? ' · ' + data.notes.join('; ') : ''}`
}

async function runDecode() {
    const hex = get('decode_hex').value
    const direction = get('decode_direction').value
    const model_id = get('decode_model').value || undefined
    try {
        const res = await fetch(`${baseUrl}api/decode`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ hex, direction, model_id }),
        })
        const data = await res.json()
        if (!data.ok) {
            M.toast({ html: data.error || 'decode failed' })
            return
        }
        renderDecode(data)
    } catch (err) {
        M.toast({ html: `decode error: ${err}` })
    }
}

async function runExport() {
    const hex = get('decode_hex').value
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
            M.toast({ html: data.error || 'export failed' })
            return
        }
        if (data.decode) renderDecode(data.decode)
        get('llm_export').value = data.text || ''
        M.toast({ html: `Export ready (${data.unknownCount || 0} unknowns)` })
    } catch (err) {
        M.toast({ html: `export error: ${err}` })
    }
}

async function copyExport() {
    const text = get('llm_export').value
    if (!text) {
        M.toast({ html: 'Nothing to copy — run Export first' })
        return
    }
    try {
        await navigator.clipboard.writeText(text)
        M.toast({ html: 'Copied LLM export to clipboard' })
    } catch {
        get('llm_export').select()
        document.execCommand('copy')
        M.toast({ html: 'Copied (fallback)' })
    }
}

get('btn_decode')?.addEventListener('click', runDecode)
get('btn_export_llm')?.addEventListener('click', runExport)
get('btn_copy_export')?.addEventListener('click', copyExport)

// ── WebSocket status ──────────────────────────────────────────────────────

let retryDelay = 250

function connect() {
    clearTimeout(reconnectTimer)
    if (ws) {
        ws.onclose = ws.onopen = ws.onmessage = null
        try {
            ws.close()
        } catch {}
    }
    ws = new WebSocket(baseUrl + 'ws')

    ws.onclose = () => {
        get('status_rethink').innerHTML = STATUS_ERROR
        get('status_mqtt').innerHTML = STATUS_UNKNOWN
        document.getElementsByTagName('body')[0].classList.add('offline')
        reconnectTimer = setTimeout(connect, retryDelay)
        retryDelay = 5000
    }

    ws.onopen = () => {
        retryDelay = 250
        get('status_rethink').innerHTML = STATUS_OK
        document.getElementsByTagName('body')[0].classList.remove('offline')
    }

    ws.onmessage = (ev) => {
        if (typeof ev.data !== 'string') return
        const json = JSON.parse(ev.data)
        if (typeof json.ha === 'boolean') {
            get('status_mqtt').innerHTML = json.ha ? STATUS_OK : STATUS_ERROR
        }

        if (typeof json.devices === 'object') {
            let deletedDevices = Object.keys(devices).filter((id) => !json.devices[id])
            deletedDevices.forEach((id) => {
                devices[id].destroy()
                delete devices[id]
            })
            for (const id in json.devices) {
                const j = json.devices[id]
                if (!devices[id]) devices[id] = new DeviceEntry(id, j, get('devices_body'))
                else devices[id].update(j)
            }
            updateDevicesEmpty()
        }

        if (typeof json.bridge === 'object') {
            bridge_status = json.bridge.loggedIn
            if (json.bridge.loggedIn === true) {
                document.getElementById('btn_thinq_login').classList.add('hide')
                document.getElementById('btn_thinq_logout').classList.remove('hide')
                get('status_bridge').innerHTML = STATUS_OK
                get('status_bridge_text').innerText = 'Ok'
            } else {
                document.getElementById('btn_thinq_login').classList.remove('hide')
                document.getElementById('btn_thinq_logout').classList.add('hide')
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

function get(id) {
    return document.getElementById(id)
}

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

updateDevicesEmpty()
connect()
