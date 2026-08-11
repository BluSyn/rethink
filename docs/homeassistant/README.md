# Home Assistant integration (rethink)

Rethink publishes appliances via **MQTT discovery** as normal Home Assistant
entities (climate, sensors, switches, …). Notifications use the same model —
**entities you can pick in the UI** — not MQTT device triggers.

## Notification entities

| Condition | Entity type | Typical name / key | Automate on |
|-----------|-------------|--------------------|-------------|
| Filter needs change (AC) | `binary_sensor` (problem) | `filter_needs_change` | state → `on` |
| Filter just became bad | `event` | `filter_changed` | event type `filter_needs_change` |
| Bucket full (dehumidifier) | `binary_sensor` (problem) | `bucket_full` | state → `on` |
| Bucket full / emptied edge | `event` | `bucket_alert` | `bucket_full` / `bucket_ok` |
| Dryer cycle finished | `event` | `cycle_complete` | event type `cycle_complete` |
| Fridge door | `binary_sensor` (door) | `door` | state → `on` / `off` |

Exact `entity_id`s depend on device name and HA’s naming rules. Open
**Settings → Devices → [your LG appliance]** and copy the entity from the list.

## Blueprints (copy / paste)

Ready-made automation blueprints live in [`blueprints/`](blueprints/):

| File | Purpose |
|------|---------|
| [`rethink_problem_notify.yaml`](blueprints/rethink_problem_notify.yaml) | Filter / bucket problem sensor → notify |
| [`rethink_event_notify.yaml`](blueprints/rethink_event_notify.yaml) | Cycle complete / bucket or filter alert event → notify |
| [`rethink_door_notify.yaml`](blueprints/rethink_door_notify.yaml) | Door open (optional delay) → notify |

### Install

1. On the Home Assistant host, create a folder (if needed):

   ```bash
   mkdir -p /config/blueprints/automation/rethink
   ```

2. Copy the YAML files into that folder (or paste file contents via the File
   editor / Samba add-on).

3. **Developer tools → YAML → Reload automations** (or restart HA).

4. **Settings → Automations & scenes → Create automation → Use a blueprint**  
   Choose a rethink blueprint, pick the entity and notify service
   (e.g. `notify.mobile_app_your_phone` or `notify.persistent_notification`).

### Without blueprints (minimal YAML)

**Problem sensor (filter / bucket):**

```yaml
alias: AC filter needs change
trigger:
  - platform: state
    entity_id: binary_sensor.lg_air_conditioner_filter_needs_change
    from: "off"
    to: "on"
action:
  - service: notify.persistent_notification
    data:
      title: Filter
      message: Air conditioner filter needs change
```

**Event (cycle complete):**

```yaml
alias: Dryer cycle complete
trigger:
  - platform: state
    entity_id: event.lg_dryer_cycle_complete
condition:
  - condition: template
    value_template: >-
      {{ trigger.to_state.attributes.event_type == 'cycle_complete' }}
action:
  - service: notify.persistent_notification
    data:
      title: Dryer
      message: Cycle complete
```

## MQTT discovery notes

- Discovery topic: `homeassistant/device/rethink/<device-uuid>/config`
- State / command topics: `rethink/<device-uuid>/…`
- Availability: device + global `rethink/availability` (mode `all`)
- Event payloads are **non-retained** JSON: `{"event_type":"…"}` on
  `rethink/<id>/events/<object_id>`

We intentionally do **not** use MQTT device_automation triggers for
notifications; they do not surface reliably in the HA automation UI.
