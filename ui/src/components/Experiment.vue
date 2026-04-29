<template>
  <div class="exp-container">
    <!-- Title Bar -->
    <div class="exp-title-bar">
      <span class="exp-icon">🧪</span>
      <input v-model="expTitle" class="exp-title-input" />
      <div class="title-spacer"></div>
      <span class="state-label" :class="mode">{{ stateText }}</span>
    </div>

    <!-- Body: sidebar + right area -->
    <div class="exp-body">
      <!-- Left sidebar -->
      <div class="exp-sidebar">
        <!-- Device connect / config group -->
        <button class="sb-btn" title="Connect / Disconnect devices" @click="openConnectModal">🔌</button>
        <button class="sb-btn" title="Experiment settings (MF4, devices)" @click="openConfigModal">⚙</button>
        <button class="sb-btn" title="Export calibration variables to DCM file" @click="openDcmExportModal" :disabled="dcmExporting">
          {{ dcmExporting ? '⏳' : '💾' }}
        </button>
        <div class="sb-sep"></div>
        <button class="sb-btn" title="Add / Remove Variables" @click="openVarModal">📋</button>
        <div class="sb-sep"></div>
        <button v-if="mode === 'idle'" class="sb-btn" title="Start DAQ" @click="startDaq">▶</button>
        <button v-if="mode === 'running'" class="sb-btn rec-btn" title="Start Recording" @click="startRecording">⏺</button>
        <button v-if="mode !== 'idle'" class="sb-btn stop-btn" title="Stop" @click="stopDaq">■</button>
        <button class="sb-btn" title="Refresh calibration values" @click="refreshCalValues">↻</button>
        <div class="sb-sep"></div>
        <div class="pm-toggle-wrap" title="Toggle page mode (Reference / Working)" @click="togglePageMode">
          <span class="pm-label" :class="{ active: pageMode === 'reference' }">R</span>
          <div class="pm-toggle" :class="{ working: pageMode === 'working' }">
            <div class="pm-thumb"></div>
          </div>
          <span class="pm-label" :class="{ active: pageMode === 'working' }">W</span>
        </div>
        <div class="sb-sep"></div>
        <button class="sb-btn" title="Verify Checksum" @click="runChecksum">✓</button>
        <button class="sb-btn" title="New Tab" @click="addTab">＋</button>
        <div class="sb-sep"></div>
        <button class="sb-btn" title="Save experiment setup to JSON" @click="saveExpSetup">🖫</button>
        <button class="sb-btn" title="Load experiment setup from JSON" @click="loadExpSetup">📂</button>
        <div class="sb-flex"></div>
        <button class="sb-btn sb-help" title="Help & user guide" @click="showHelpModal = true">❓</button>
      </div>

      <!-- Right: tabs + content -->
      <div class="exp-right">
        <!-- Tab bar -->
        <div class="exp-tab-bar">
          <div v-for="tab in tabs" :key="tab.id"
            class="exp-tab" :class="{ active: activeTabId === tab.id }"
            @click="selectTab(tab.id)">
            <span class="tab-title" @dblclick.stop="startRename(tab)">{{ tab.title }}</span>
            <span class="tab-close" @click.stop="closeTab(tab.id)">×</span>
          </div>
          <button class="tab-add-btn" @click="addTab">+</button>
        </div>

        <!-- Content area -->
        <div class="exp-content">
          <!-- Empty state -->
          <div v-if="!activeTab || activeTab.variables.length === 0" class="exp-empty">
            <div class="empty-icon">📋</div>
            <p>No variables in this tab</p>
            <p class="empty-hint">Click the 📋 button in the sidebar to add measurements or calibration parameters</p>
            <button class="exp-btn primary" @click="openVarModal">📋 Add Variables</button>
          </div>

          <template v-else>
            <!-- MEASUREMENTS section -->
            <div v-if="activeMeasurements.length > 0" class="exp-section">
              <div class="section-hdr">
                <span class="section-dot meas"></span>
                MEASUREMENTS
                <span class="section-count">{{ activeMeasurements.length }}</span>
              </div>
              <div class="var-cards-grid">
                <div v-for="v in activeMeasurements" :key="varKey(v)"
                  class="var-card meas-card" :title="`${v.device_name} / ${v.name}`">
                  <div class="card-header">
                    <span class="card-device">{{ v.device_name }}</span>
                    <div class="card-actions">
                      <span class="card-info-btn" @click.stop="showInfo(v)">ℹ</span>
                      <span class="card-remove" @click.stop="removeVar(v)">✕</span>
                    </div>
                  </div>
                  <div class="card-name">{{ v.name }}</div>
                  <div class="card-value">
                    <span class="val-num">{{ getLiveValue(v) }}</span>
                    <span v-if="v.unit" class="val-unit">{{ v.unit }}</span>
                  </div>
                </div>
              </div>
            </div>

            <!-- CALIBRATION scalars section -->
            <div v-if="activeCalScalars.length > 0" class="exp-section">
              <div class="section-hdr">
                <span class="section-dot cal"></span>
                CALIBRATION
                <span class="section-count">{{ activeCalScalars.length }}</span>
                <span class="pm-indicator" :class="pageMode">
                  {{ pageMode === 'working' ? '✎ Working' : '🔒 Reference' }}
                </span>
              </div>
              <div class="var-cards-grid">
                <div v-for="v in activeCalScalars" :key="varKey(v)"
                  class="var-card cal-card"
                  :class="{ editable: pageMode === 'working', readonly: pageMode === 'reference' }"
                  :title="`${v.device_name} / ${v.name}\nClick to edit (Working mode only)`"
                  @click="pageMode === 'working' && editCal(v)">
                  <div class="card-header">
                    <span class="card-device">{{ v.device_name }}</span>
                    <div class="card-actions">
                      <span class="card-info-btn" @click.stop="showInfo(v)">ℹ</span>
                      <span class="card-remove" @click.stop="removeVar(v)">✕</span>
                    </div>
                  </div>
                  <div class="card-name">{{ v.name }}</div>
                  <div class="card-value">
                    <span class="val-num">{{ calValues[calKey(v)] ?? '—' }}</span>
                    <span class="change-mark" :class="getChangeMark(v)">{{ getChangeMarkChar(v) }}</span>
                    <span v-if="v.unit" class="val-unit">{{ v.unit }}</span>
                  </div>
                </div>
              </div>
            </div>

            <!-- MAPS & CURVES section -->
            <div v-if="activeMapCurves.length > 0" class="exp-section">
              <div class="section-hdr">
                <span class="section-dot cal"></span>
                MAPS &amp; CURVES
                <span class="section-count">{{ activeMapCurves.length }}</span>
              </div>
              <div class="var-cards-grid">
                <div v-for="v in activeMapCurves" :key="varKey(v)"
                  class="var-card cal-card map-card" :title="`${v.device_name} / ${v.name}`">
                  <div class="card-header">
                    <span class="card-device">{{ v.device_name }}</span>
                    <span class="card-remove" @click.stop="removeVar(v)">✕</span>
                  </div>
                  <div class="card-name">{{ v.name }}</div>
                  <div class="card-type">{{ v.calType }}</div>
                  <div class="map-actions">
                    <button class="exp-btn sm" @click.stop="showInfo(v)">ℹ Info</button>
                    <button class="exp-btn sm" @click.stop="showMatrix(v)">📊 Table</button>
                  </div>
                </div>
              </div>
            </div>
          </template>
        </div>
      </div>
    </div>

    <!-- Status bar -->
    <div class="exp-status-bar">
      <template v-if="statusDevices.length === 0">
        <span class="stat-item muted">No devices loaded</span>
      </template>
      <template v-for="(d, i) in statusDevices" :key="d.device_id">
        <span v-if="i > 0" class="stat-sep">|</span>
        <span class="stat-item">
          <span class="stat-dot" :class="d.connected ? 'ok' : 'err'">●</span>
          {{ d.device_name }}
          <span v-if="d.daq_running" class="daq-running">● DAQ</span>
          <span v-if="d.recording" class="daq-recording">⏺ REC</span>
        </span>
      </template>
      <span class="stat-spacer"></span>
      <span v-if="toastMsg" class="stat-toast" :class="toastErr ? 'err' : 'ok'">{{ toastMsg }}</span>
      <span v-if="statusDevices.length > 0" class="stat-pm" :class="pageMode">
        {{ pageMode === 'working' ? '✎ WORKING' : '🔒 REFERENCE' }}
      </span>
    </div>

    <!-- ===== MODALS ===== -->

    <!-- Variable selection modal -->
    <div v-if="showVarModal" class="modal-overlay" @click.self="showVarModal = false">
      <div class="modal modal-xl">
        <div class="modal-hdr">
          <h3>Select Variables</h3>
          <button class="modal-close" @click="showVarModal = false">×</button>
        </div>
        <div class="modal-filters">
          <input v-model="varSearch" class="exp-inp" placeholder="Search by name…" />
          <select v-model="varDeviceFilter" class="exp-sel">
            <option value="">All Devices</option>
            <option v-for="d in varDeviceList" :key="d.id" :value="d.id">{{ d.name }}</option>
          </select>
          <select v-model="varTypeFilter" class="exp-sel">
            <option value="">All Types</option>
            <option value="measurement">Measurements</option>
            <option value="calibration">Calibration</option>
          </select>
        </div>
        <div class="modal-body">
          <table class="var-table">
            <thead>
              <tr>
                <th style="width:32px">
                  <input type="checkbox" @change="toggleSelectAll($event.target.checked)" />
                </th>
                <th>Name</th>
                <th>Device</th>
                <th>Kind</th>
                <th>Type</th>
                <th>Address</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="v in filteredVarList" :key="varModalKey(v)"
                :class="{ selected: pendingVarsSet.has(varModalKey(v)) }"
                @click="togglePending(v)">
                <td><input type="checkbox" :checked="pendingVarsSet.has(varModalKey(v))" @click.stop @change="togglePending(v)" /></td>
                <td class="vt-name">{{ v.name }}</td>
                <td class="vt-dev">{{ v.device_name }}</td>
                <td>
                  <span class="vt-kind" :style="{ color: v.kind === 'measurement' ? '#00cc44' : '#ffcc00' }">
                    {{ v.kind === 'measurement' ? '📊 Meas' : '🔧 Cal' }}
                  </span>
                </td>
                <td class="vt-type">{{ v.kind === 'measurement' ? v.type : (v.calType + ' / ' + v.type) }}</td>
                <td class="vt-addr mono">{{ v.address_hex }}</td>
              </tr>
            </tbody>
          </table>
        </div>
        <div class="modal-footer">
          <span class="modal-count">{{ pendingVarsSet.size }} selected</span>
          <button class="exp-btn" @click="showVarModal = false">Cancel</button>
          <button class="exp-btn primary" @click="applyVarSelection">Apply</button>
        </div>
      </div>
    </div>

    <!-- Cal edit modal -->
    <div v-if="calEditTarget" class="modal-overlay" @click.self="calEditTarget = null">
      <div class="modal" style="width:380px">
        <div class="modal-hdr">
          <h3>✎ {{ calEditTarget.name }}</h3>
          <button class="modal-close" @click="calEditTarget = null">×</button>
        </div>
        <div class="modal-body">
          <div class="field-row">
            <span class="field-label">Current:</span>
            <span class="bright">{{ calValues[calKey(calEditTarget)] ?? '—' }}</span>
            <span v-if="calEditTarget.unit" class="val-unit">{{ calEditTarget.unit }}</span>
          </div>
          <div style="margin-top:12px">
            <select v-if="calEditTarget.is_enum && calEditTarget.enum_options?.length"
              v-model="calEditValue" class="exp-inp" style="width:100%">
              <option v-for="opt in calEditTarget.enum_options" :key="opt.raw" :value="opt.text">
                {{ opt.text }}
              </option>
            </select>
            <input v-else v-model="calEditValue" class="exp-inp" style="width:100%"
              placeholder="New value" @keyup.enter="saveCal" />
          </div>
        </div>
        <div class="modal-footer">
          <button class="exp-btn" @click="calEditTarget = null">Cancel</button>
          <button class="exp-btn primary" @click="saveCal" :disabled="calWriting">
            {{ calWriting ? 'Writing…' : 'Write' }}
          </button>
        </div>
        <div v-if="calEditMsg" class="modal-msg" :class="calEditErr ? 'msg-err' : 'msg-ok'">{{ calEditMsg }}</div>
      </div>
    </div>

    <!-- Info modal -->
    <div v-if="infoTarget" class="modal-overlay" @click.self="infoTarget = null">
      <div class="modal" style="width:520px">
        <div class="modal-hdr">
          <h3>ℹ {{ infoTarget.name }}</h3>
          <button class="modal-close" @click="infoTarget = null">×</button>
        </div>
        <div class="modal-body">
          <div v-if="infoLoading" class="loading">Loading…</div>
          <table v-else-if="infoData" class="info-table">
            <tr v-for="(val, key) in infoData" :key="key">
              <td class="info-key">{{ key }}</td>
              <td>{{ formatInfoVal(val) }}</td>
            </tr>
          </table>
        </div>
        <div class="modal-footer">
          <button class="exp-btn" @click="infoTarget = null">Close</button>
        </div>
      </div>
    </div>

    <!-- Matrix / Table modal -->
    <div v-if="matrixTarget" class="modal-overlay" @click.self="matrixTarget = null">
      <div class="modal matrix-modal">
        <div class="modal-hdr">
          <div>
            <h3>{{ matrixTarget.name }}</h3>
            <div class="matrix-meta">
              {{ matrixTarget.device_name }} | {{ matrixTarget.calType }} | {{ matrixRows }}×{{ matrixCols }}
              <template v-if="matrixTransposed">
                <span class="axis-label"> | X: {{ matrixXAxisName }} | Y: {{ matrixYAxisName }}</span>
              </template>
              <template v-else-if="matrixXAxisName">
                <span class="axis-label"> | {{ matrixXAxisName }}</span>
              </template>
            </div>
          </div>
          <button class="modal-close" @click="matrixTarget = null">×</button>
        </div>
        <div class="modal-body">
          <div v-if="matrixLoading" class="loading">Loading…</div>
          <div v-else class="matrix-scroll">
            <table class="matrix-table">
              <thead>
                <tr>
                  <!-- corner cell: Y-axis name for 2D, or empty for 1D -->
                  <th class="matrix-corner">{{ matrixTransposed ? "" : '' }}</th>
                  <th v-for="ci in matrixCols" :key="ci" class="matrix-col-hdr">
                    {{ matrixXAxisValues[ci - 1] ?? (ci - 1) }}
                  </th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="(row, ri) in matrixData" :key="ri">
                  <!-- row header: Y-axis value for 2D, index for 1D -->
                  <td class="matrix-ri">{{ matrixTransposed ? (matrixYAxisValues[ri] ?? ri) : ri }}</td>
                  <td v-for="(cell, ci) in row" :key="ci">
                    <input class="matrix-cell" :value="cell"
                      :disabled="pageMode !== 'working'"
                      @change="e => writeCell(ri, ci, e.target.value)" />
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>
        <div class="modal-footer">
          <span v-if="matrixMsg" class="modal-msg-inline" :class="matrixErr ? 'msg-err' : 'msg-ok'">{{ matrixMsg }}</span>
          <button class="exp-btn" @click="matrixTarget = null">Close</button>
        </div>
      </div>
    </div>

    <!-- Checksum modal -->
    <div v-if="showChecksumModal" class="modal-overlay" @click.self="showChecksumModal = false">
      <div class="modal" style="width:900px;max-width:95vw">
        <div class="modal-hdr">
          <h3>Checksum Verification</h3>
          <button class="modal-close" @click="showChecksumModal = false">×</button>
        </div>
        <div class="modal-body">
          <div v-if="checksumLoading" class="loading">Running verification…</div>
          <template v-else>
            <div v-if="checksumResults.length === 0" class="loading muted">
              No results (ensure HEX files are loaded for connected devices).
            </div>
            <div v-for="dev in checksumResults" :key="dev.device_id" class="chk-device">
              <div class="chk-dev-hdr" :class="dev.passed ? 'pass' : 'fail'">
                <strong>{{ dev.device_name }}</strong>
                <span class="chk-status">
                  {{ dev.passed ? '✓ ALL PASS' : (dev.error ? '✗ ERROR' : '✗ MISMATCH') }}
                </span>
              </div>
              <div v-if="dev.error" class="chk-error">{{ dev.error }}</div>
              <table v-else-if="dev.segments?.length" class="chk-table">
                <thead>
                  <tr><th>Name</th><th>Address</th><th>Expected</th><th>Actual</th><th>Match</th></tr>
                </thead>
                <tbody>
                  <tr v-for="s in dev.segments" :key="s.address" :class="s.match ? 'ok-row' : 'fail-row'">
                    <td>{{ s.name }}</td>
                    <td class="mono">{{ s.address }}</td>
                    <td class="mono">{{ s.master_checksum }}</td>
                    <td class="mono">{{ s.slave_checksum }}</td>
                    <td class="chk-match">{{ s.match ? '✓' : '✗' }}</td>
                  </tr>
                </tbody>
              </table>
            </div>
          </template>
        </div>
        <div class="modal-footer">
          <button class="exp-btn" @click="showChecksumModal = false">Close</button>
        </div>
      </div>
    </div>

    <!-- Connect / Disconnect devices modal -->
    <div v-if="showConnectModal" class="modal-overlay" @click.self="showConnectModal = false">
      <div class="modal" style="width:680px;max-width:95vw">
        <div class="modal-hdr">
          <h3>🔌 Device Connections</h3>
          <button class="modal-close" @click="showConnectModal = false">×</button>
        </div>
        <div class="modal-body">
          <div v-if="connectDevices.length === 0" class="loading muted">
            No devices configured. Go to ⚙ Config to add a device.
          </div>
          <div v-for="dev in connectDevices" :key="dev.id" class="conn-device-row">
            <div class="conn-device-info">
              <span class="conn-dot" :class="dev.connected ? 'on' : 'off'"></span>
              <div class="conn-device-text">
                <div class="conn-device-name">{{ dev.name }}</div>
                <div class="conn-device-addr">{{ dev.address || '—' }}</div>
              </div>
            </div>
            <div class="conn-device-status">
              <span class="tag" :class="dev.connected ? 'connected' : 'disconnected'">
                {{ dev.connected ? 'Connected' : 'Disconnected' }}
              </span>
            </div>
            <div class="conn-device-actions">
              <select v-if="!dev.connected" v-model="connectTransport[dev.id]" class="transport-select">
                <option v-for="opt in dev.transport_options" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
              </select>
              <button v-if="!dev.connected"
                class="exp-btn primary"
                :disabled="connectBusy[dev.id]"
                @click="connectDev(dev)">
                {{ connectBusy[dev.id] ? 'Connecting…' : 'Connect' }}
              </button>
              <button v-else
                class="exp-btn stop-btn"
                :disabled="connectBusy[dev.id]"
                @click="disconnectDev(dev)">
                {{ connectBusy[dev.id] ? 'Disconnecting…' : 'Disconnect' }}
              </button>
            </div>
          </div>
        </div>
        <div class="modal-footer">
          <button class="exp-btn primary" @click="connectAllDevs" :disabled="connectAllBusy">
            {{ connectAllBusy ? 'Connecting…' : '⚡ Connect All' }}
          </button>
          <button class="exp-btn" @click="showConnectModal = false">Close</button>
        </div>
      </div>
    </div>

    <!-- Experiment Config modal -->
    <div v-if="showConfigModal" class="modal-overlay" @click.self="showConfigModal = false">
      <div class="modal" style="width:560px;max-width:95vw">
        <div class="modal-hdr">
          <h3>⚙ Experiment Settings</h3>
          <button class="modal-close" @click="showConfigModal = false">×</button>
        </div>
        <div class="modal-body">
          <div class="form-group">
            <label class="form-label">MF4 File Name Pattern</label>
            <input class="form-input mono" v-model="mf4Pattern" placeholder="{title}_{YYYYMMDD}_{HHMMSS}.mf4">
            <div class="form-hint">Placeholders: {title}, {YYYYMMDD}, {HHMMSS}</div>
          </div>
          <div class="form-group">
            <label class="form-label">MF4 Output Directory</label>
            <input class="form-input" v-model="mf4Dir" placeholder="records/">
          </div>
          <div class="form-group" v-if="statusDevices.length > 0">
            <label class="form-label">Devices</label>
            <div class="cfg-dev-list">
              <div v-for="d in statusDevices" :key="d.device_id" class="cfg-dev-row">
                <span class="conn-dot" :class="{ connected: d.connected, disconnected: !d.connected }"></span>
                <input class="form-input cfg-dev-name" :value="d.device_name"
                  @change="renameDevice(d.device_id, $event.target.value)"
                  :title="`Rename device (current: ${d.device_name})`">
              </div>
            </div>
            <div class="form-hint">Click device name to rename</div>
          </div>
          <div class="cfg-dev-link">
            <a href="#" @click.prevent="goToDevMgr">→ Device Manager (A2L, HEX files)</a>
          </div>
        </div>
        <div class="modal-footer">
          <button class="exp-btn primary" @click="saveConfig">✓ Save</button>
          <button class="exp-btn" @click="showConfigModal = false">Cancel</button>
        </div>
      </div>
    </div>

    <!-- DCM Export Variable Picker modal -->
    <div v-if="showDcmModal" class="modal-overlay" @click.self="showDcmModal = false">
      <div class="modal" style="width:700px;max-width:95vw">
        <div class="modal-hdr">
          <h3>📤 Export Calibrations to DCM</h3>
          <button class="modal-close" @click="showDcmModal = false">×</button>
        </div>
        <div class="modal-body">
          <p class="muted" style="margin-bottom:10px;font-size:13px;">
            All calibration variables in this experiment are listed below.
            Variables modified during this session are marked <span style="color:#ffcc00;font-weight:700;">● MODIFIED</span>.
            Select which to include in the DCM export.
          </p>
          <div class="dcm-sel-bar">
            <label style="display:flex;align-items:center;gap:6px;cursor:pointer;font-size:13px;color:#aaa;">
              <input type="checkbox" v-model="dcmSelectAll" @change="toggleDcmAll"> Select All
            </label>
            <span class="muted" style="font-size:12px;">{{ dcmSelectedCount }}/{{ dcmTotalCount }} selected</span>
          </div>
          <div v-if="dcmVarGroups.length === 0" class="muted" style="text-align:center;padding:24px;">
            No calibration variables in the experiment.
          </div>
          <div v-for="group in dcmVarGroups" :key="group.device_id" class="dcm-group">
            <div class="dcm-group-hdr">{{ group.device_name }}</div>
            <label v-for="v in group.variables" :key="v.key"
              class="dcm-var-row"
              :class="{ modified: v.is_changed }">
              <input type="checkbox" v-model="dcmSelected[v.key]" @change="updateDcmCount">
              <div class="dcm-var-info">
                <span class="dcm-var-name">{{ v.name }}</span>
                <span class="dcm-var-type">[{{ v.calType }}]</span>
                <span v-if="v.is_changed" class="dcm-modified-badge">● MODIFIED</span>
                <div v-if="v.changeDetail" class="dcm-change-detail">{{ v.changeDetail }}</div>
              </div>
            </label>
          </div>
        </div>
        <div class="modal-footer">
          <button class="exp-btn primary" @click="doDcmExport" :disabled="dcmExporting">
            {{ dcmExporting ? '⏳ Generating…' : '📤 Export DCM' }}
          </button>
          <button class="exp-btn" @click="showDcmModal = false">Cancel</button>
        </div>
      </div>
    </div>

    <!-- Device Mapping modal (shown during experiment load when devices don't match) -->
    <div v-if="showDeviceMapModal" class="modal-overlay">
      <div class="modal" style="width:700px;max-width:95vw">
        <div class="modal-hdr">
          <h3>📂 Map Saved Devices</h3>
          <button class="modal-close" @click="cancelDeviceMap">×</button>
        </div>
        <div class="modal-body">
          <p class="muted" style="margin-bottom:12px;">
            Some saved devices could not be automatically matched to currently connected devices.
            Select a mapping below, or leave blank to drop those variables.
          </p>
          <div v-for="row in deviceMapRows" :key="row.savedId" class="devmap-row">
            <div class="devmap-saved">
              <div class="devmap-name">{{ row.savedName }}</div>
              <div class="devmap-id">{{ row.savedId }}</div>
            </div>
            <span class="devmap-arrow">→</span>
            <select class="transport-select devmap-sel" v-model="deviceMapSelections[row.savedId]">
              <option value="">(Skip – drop variables)</option>
              <option v-for="d in row.currentDevices" :key="d.device_id" :value="d.device_id">
                {{ d.device_name }} {{ d.connected ? '✓' : '(disconnected)' }}
              </option>
            </select>
          </div>
        </div>
        <div class="modal-footer">
          <button class="exp-btn primary" @click="applyDeviceMap">✓ Apply & Load</button>
          <button class="exp-btn" @click="cancelDeviceMap">Cancel</button>
        </div>
      </div>
    </div>

    <!-- Help modal -->
    <div v-if="showHelpModal" class="modal-overlay" @click.self="showHelpModal = false">
      <div class="modal" style="width:860px;max-width:95vw;max-height:85vh">
        <div class="modal-hdr">
          <h3>❓ Experiment Page — User Guide</h3>
          <button class="modal-close" @click="showHelpModal = false">×</button>
        </div>
        <div class="modal-body" style="overflow-y:auto;max-height:calc(85vh - 100px)">
          <div class="help-section">
            <h4 class="help-h4 green">⚡ Quick Start</h4>
            <ol class="help-ol">
              <li>Click <strong>🔌</strong> to connect XCP devices (or use ⚙ → Device Manager)</li>
              <li>Click <strong>📋</strong> to select measurements and calibration parameters</li>
              <li>Click <strong>▶</strong> to start DAQ data acquisition</li>
              <li>Click <strong>⏺</strong> to start MF4 recording (optional)</li>
              <li>Click <strong>■</strong> to stop everything</li>
            </ol>
          </div>
          <div class="help-section">
            <h4 class="help-h4">🔧 Sidebar Buttons</h4>
            <table class="help-table">
              <tbody>
                <tr><td class="help-btn">🔌</td><td>Open device connection / disconnect dialog</td></tr>
                <tr><td class="help-btn">⚙</td><td>Navigate to Device Manager (A2L, HEX, rename)</td></tr>
                <tr><td class="help-btn">💾</td><td>Export <em>all cached calibration values</em> to a DCM file (pick save path)</td></tr>
                <tr><td class="help-btn">📋</td><td>Open variable selection: add measurements & calibration parameters to the current tab</td></tr>
                <tr><td class="help-btn">▶</td><td>Start DAQ acquisition on all devices that have selected measurements</td></tr>
                <tr><td class="help-btn">⏺</td><td>Start MF4 recording (only shown when DAQ is running)</td></tr>
                <tr><td class="help-btn">■</td><td>Stop DAQ and/or recording</td></tr>
                <tr><td class="help-btn">↻</td><td>Refresh calibration values from all connected devices</td></tr>
                <tr><td class="help-btn">R/W</td><td>Toggle between Reference and Working calibration page. Editing only works in Working mode</td></tr>
                <tr><td class="help-btn">✓</td><td>Run checksum verification against loaded HEX files</td></tr>
                <tr><td class="help-btn">＋</td><td>Add a new tab</td></tr>
                <tr><td class="help-btn">🖫</td><td>Save current experiment setup (tabs, variables) to a JSON file</td></tr>
                <tr><td class="help-btn">📂</td><td>Load experiment setup from a JSON file (DAQ must be stopped)</td></tr>
              </tbody>
            </table>
          </div>
          <div class="help-section">
            <h4 class="help-h4">📊 Variable Cards</h4>
            <ul class="help-ul">
              <li><strong style="color:#00cc44">MEASUREMENT</strong> — blue border. Shows live DAQ value when running. Click ✕ to remove.</li>
              <li><strong style="color:#ff9944">CALIBRATION</strong> — orange border. Shows cached value with ▲▼ change indicators. Click card to edit (Working mode only). ℹ shows detail.</li>
              <li><strong style="color:#ff9944">MAPS &amp; CURVES</strong> — orange border. Info (ℹ) shows axis data; Table opens matrix editor.</li>
            </ul>
          </div>
          <div class="help-section">
            <h4 class="help-h4">🔄 State Machine</h4>
            <div class="help-states">
              <div class="help-state idle">💤 IDLE<br><small>Edit freely</small></div>
              <span class="help-arrow">→</span>
              <div class="help-state running">▶ RUNNING<br><small>Live values</small></div>
              <span class="help-arrow">→</span>
              <div class="help-state recording">⏺ RECORDING<br><small>Writing MF4</small></div>
            </div>
          </div>
          <div class="help-section">
            <h4 class="help-h4">💡 Tips</h4>
            <ul class="help-ul">
              <li>Double-click a tab title to rename it</li>
              <li>Variables are remembered across sessions (localStorage)</li>
              <li>Use 🖫 Save to backup your setup and 📂 Load to restore on another session</li>
              <li>DAQ collects measurements from <em>all tabs</em>, not just the active one</li>
              <li>DCM export writes <em>all currently cached</em> calibration values — read them first with ↻</li>
            </ul>
          </div>
        </div>
        <div class="modal-footer">
          <button class="exp-btn" @click="showHelpModal = false">Close</button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, reactive, computed, watch, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { save, open } from '@tauri-apps/plugin-dialog'
import { useRouter } from 'vue-router'

const STORAGE_KEY = 'exp_tauri_state_v3'

// ── Core state ────────────────────────────────────────────────────────────────
const expTitle = ref('Experiment')
const tabs = ref([])
const activeTabId = ref(null)
const tabCounter = ref(0)

const allMeasurements = ref([])
const allCalibrations = ref([])

// live values: "device_id:name" → number
const liveValues = reactive({})
// cal values: "device_id:idx" → string
const calValues = reactive({})
// original cal values: "device_id:idx" → string (for change marks)
const calOriginals = reactive({})

const mode = ref('idle')     // 'idle' | 'running' | 'recording'
const pageMode = ref('reference')
const statusDevices = ref([])

// ── Variable modal state ──────────────────────────────────────────────────────
const showVarModal = ref(false)
const varSearch = ref('')
const varDeviceFilter = ref('')
const varTypeFilter = ref('')
const pendingVarsSet = reactive(new Set())

// ── Cal edit modal state ──────────────────────────────────────────────────────
const calEditTarget = ref(null)
const calEditValue = ref('')
const calWriting = ref(false)
const calEditMsg = ref('')
const calEditErr = ref(false)

// ── Info modal state ──────────────────────────────────────────────────────────
const infoTarget = ref(null)
const infoLoading = ref(false)
const infoData = ref(null)

// ── Matrix modal state ────────────────────────────────────────────────────────
const matrixTarget = ref(null)
const matrixLoading = ref(false)
const matrixData = ref([])
const matrixRows = ref(1)
const matrixCols = ref(1)
const matrixMsg = ref('')
const matrixErr = ref(false)
const matrixXAxisValues = ref([])  // X-axis physical values (column headers)
const matrixYAxisValues = ref([])  // Y-axis physical values (row headers)
const matrixXAxisName = ref('')
const matrixYAxisName = ref('')
const matrixTransposed = ref(false) // true for 2D MAP (x/y transposed for display)

// ── Checksum modal state ──────────────────────────────────────────────────────
const showChecksumModal = ref(false)
const checksumLoading = ref(false)
const checksumResults = ref([])

// ── Connect modal state ───────────────────────────────────────────────────────
const showConnectModal = ref(false)
const connectDevices = ref([])
const connectTransport = reactive({})
const connectBusy = reactive({})
const connectAllBusy = ref(false)
const dcmExporting = ref(false)

// ── Config modal state ────────────────────────────────────────────────────────
const showConfigModal = ref(false)
const mf4Pattern = ref('{title}_{YYYYMMDD}_{HHMMSS}.mf4')
const mf4Dir = ref('records/')

// ── DCM Export picker state ───────────────────────────────────────────────────
const showDcmModal = ref(false)
const dcmVarGroups = ref([])     // [{device_id, device_name, variables:[{key,name,calType,is_changed,changeDetail}]}]
const dcmSelected = reactive({}) // key → bool
const dcmSelectAll = ref(true)
const dcmSelectedCount = ref(0)
const dcmTotalCount = ref(0)
// Track which cal params were modified this session: key = "device_id:name" → {original_value, current_value}
const changedParams = reactive({})

// ── Save/Load state ───────────────────────────────────────────────────────────
const showDeviceMapModal = ref(false)
const deviceMapRows = ref([])           // [{savedId, savedName, currentDevices: []}]
const deviceMapSelections = reactive({})
let pendingLoadData = null

// ── Help modal state ──────────────────────────────────────────────────────────
const showHelpModal = ref(false)

const router = useRouter()

// ── Toast ─────────────────────────────────────────────────────────────────────
const toastMsg = ref('')
const toastErr = ref(false)
let toastTimer = null

let statusPollInterval = null
let daqUnlisten = null

// ── Computed ──────────────────────────────────────────────────────────────────
const activeTab = computed(() => tabs.value.find(t => t.id === activeTabId.value) ?? null)

const activeMeasurements = computed(() =>
  activeTab.value?.variables.filter(v => v.kind === 'measurement') ?? [])

const activeCalScalars = computed(() =>
  activeTab.value?.variables.filter(v => v.kind === 'calibration' && !v.isMapCurve) ?? [])

const activeMapCurves = computed(() =>
  activeTab.value?.variables.filter(v => v.kind === 'calibration' && v.isMapCurve) ?? [])

const stateText = computed(() => {
  if (mode.value === 'recording') return '⏺ RECORDING'
  if (mode.value === 'running') return '▶ RUNNING'
  return '○ IDLE'
})

const varDeviceList = computed(() => {
  const seen = new Set()
  const list = []
  for (const v of [...allMeasurements.value, ...allCalibrations.value]) {
    if (!seen.has(v.device_id)) {
      seen.add(v.device_id)
      list.push({ id: v.device_id, name: v.device_name })
    }
  }
  return list
})

const varModalCombined = computed(() => {
  const meas = allMeasurements.value.map(v => ({ ...v, kind: 'measurement' }))
  const cal = allCalibrations.value.map(v => ({
    ...v,
    kind: 'calibration',
    calType: v.characteristic_type || v.type || 'VALUE',
    isMapCurve: checkIsMapCurve(v),
  }))
  return [...meas, ...cal]
})

const filteredVarList = computed(() => {
  return varModalCombined.value.filter(v => {
    if (varSearch.value && !v.name.toLowerCase().includes(varSearch.value.toLowerCase())) return false
    if (varDeviceFilter.value && v.device_id !== varDeviceFilter.value) return false
    if (varTypeFilter.value && v.kind !== varTypeFilter.value) return false
    return true
  })
})

// ── Helpers ───────────────────────────────────────────────────────────────────
function varKey(v) { return `${v.kind}:${v.device_id}:${v.idx}` }
function calKey(v) { return `${v.device_id}:${v.idx}` }
function varModalKey(v) { return `${v.kind}:${v.device_id}:${v.idx}` }

function checkIsMapCurve(v) {
  const ct = (v.characteristic_type || '').toUpperCase()
  const tn = (v.type || '').toLowerCase()
  return ['MAP', 'CURVE', 'VAL_BLK', 'CUBOID', 'CUBE_4', 'CUBE_5'].includes(ct) ||
    tn.includes('matrix') || tn.includes('array')
}

function getLiveValue(v) {
  const val = liveValues[`${v.device_id}:${v.name}`]
  if (val === undefined || val === null) return '—'
  if (typeof val === 'number') {
    return Number.isInteger(val) ? String(val) : val.toFixed(4).replace(/\.?0+$/, '')
  }
  return String(val)
}

function getChangeMark(v) {
  const cur = parseFloat(calValues[calKey(v)])
  const orig = parseFloat(calOriginals[calKey(v)])
  if (isNaN(cur) || isNaN(orig)) return ''
  if (cur > orig) return 'mark-up'
  if (cur < orig) return 'mark-down'
  return ''
}

function getChangeMarkChar(v) {
  const cur = parseFloat(calValues[calKey(v)])
  const orig = parseFloat(calOriginals[calKey(v)])
  if (isNaN(cur) || isNaN(orig)) return ''
  if (cur > orig) return '▲'
  if (cur < orig) return '▼'
  return ''
}

function formatInfoVal(val) {
  if (Array.isArray(val)) return JSON.stringify(val)
  return String(val ?? '—')
}

function toast(msg, err = false) {
  toastMsg.value = msg
  toastErr.value = err
  clearTimeout(toastTimer)
  toastTimer = setTimeout(() => { toastMsg.value = '' }, 4000)
}

// cal device_id for commands that need config dev.id (cal_read / cal_write etc.)
function configId(v) { return v.config_device_id || v.device_id }

// ── Tab management ────────────────────────────────────────────────────────────
function addTab() {
  const id = `tab-${Date.now()}`
  tabs.value.push({ id, title: `Tab ${++tabCounter.value}`, variables: [] })
  activeTabId.value = id
  saveState()
}

function closeTab(id) {
  const idx = tabs.value.findIndex(t => t.id === id)
  if (idx < 0) return
  tabs.value.splice(idx, 1)
  if (activeTabId.value === id) {
    activeTabId.value = tabs.value[Math.min(idx, tabs.value.length - 1)]?.id ?? null
  }
  if (tabs.value.length === 0) addTab()
  else saveState()
}

function selectTab(id) { activeTabId.value = id }

function startRename(tab) {
  const newTitle = prompt('Rename tab:', tab.title)
  if (newTitle && newTitle.trim()) {
    tab.title = newTitle.trim()
    saveState()
  }
}

// ── Variable modal ────────────────────────────────────────────────────────────
function openVarModal() {
  pendingVarsSet.clear()
  if (activeTab.value) {
    for (const v of activeTab.value.variables) pendingVarsSet.add(varKey(v))
  }
  varSearch.value = ''
  varDeviceFilter.value = ''
  varTypeFilter.value = ''
  showVarModal.value = true
}

function toggleSelectAll(checked) {
  for (const v of filteredVarList.value) {
    const key = varModalKey(v)
    if (checked) pendingVarsSet.add(key)
    else pendingVarsSet.delete(key)
  }
}

function togglePending(v) {
  const key = varModalKey(v)
  if (pendingVarsSet.has(key)) pendingVarsSet.delete(key)
  else pendingVarsSet.add(key)
}

function applyVarSelection() {
  if (!activeTab.value) return
  activeTab.value.variables = varModalCombined.value
    .filter(v => pendingVarsSet.has(varModalKey(v)))
    .map(v => ({ ...v }))
  showVarModal.value = false
  saveState()
  // Snapshot originals for newly added cal vars
  const calVars = activeTab.value.variables.filter(v => v.kind === 'calibration')
  snapshotOriginals(calVars)
}

function removeVar(v) {
  if (!activeTab.value) return
  const key = varKey(v)
  activeTab.value.variables = activeTab.value.variables.filter(x => varKey(x) !== key)
  saveState()
}

// ── DAQ control ───────────────────────────────────────────────────────────────
async function startDaq() {
  const selections = {}
  for (const tab of tabs.value) {
    for (const v of tab.variables) {
      if (v.kind !== 'measurement') continue
      if (!selections[v.device_id]) selections[v.device_id] = []
      if (!selections[v.device_id].includes(v.idx)) selections[v.device_id].push(v.idx)
    }
  }
  if (Object.keys(selections).length === 0) {
    toast('No measurements selected across any tab', true)
    return
  }
  try {
    const res = await invoke('experiment_start', { config: { selections } })
    if (res.success || res.started > 0) {
      mode.value = 'running'
      toast(`DAQ started on ${res.started} device(s)`)
    } else {
      toast(`Start failed: ${res.errors?.join(', ') || 'Unknown error'}`, true)
    }
  } catch (e) { toast(String(e), true) }
}

async function startRecording() {
  try {
    const res = await invoke('experiment_record_start', {
      config: { title: expTitle.value, dir: 'records' }
    })
    if (res.success || res.started > 0) {
      mode.value = 'recording'
      toast(`Recording started on ${res.started} device(s)`)
    } else {
      toast(`Record failed: ${res.errors?.join(', ')}`, true)
    }
  } catch (e) { toast(String(e), true) }
}

async function stopDaq() {
  try {
    if (mode.value === 'recording') await invoke('experiment_record_stop')
    const res = await invoke('experiment_stop')
    mode.value = 'idle'
    toast(`Stopped ${res.stopped ?? 0} device(s)`)
  } catch (e) { toast(String(e), true) }
}

// ── Calibration ───────────────────────────────────────────────────────────────
async function refreshCalValues() {
  const calVars = []
  const seen = new Set()
  for (const tab of tabs.value) {
    for (const v of tab.variables) {
      if (v.kind === 'calibration' && !seen.has(calKey(v))) {
        seen.add(calKey(v))
        calVars.push(v)
      }
    }
  }
  let count = 0
  for (const v of calVars) {
    try {
      const r = await invoke('cal_read', { deviceId: configId(v), idx: v.idx })
      if (r.success) {
        calValues[calKey(v)] = (r.is_enum && r.display_value) ? r.display_value : r.value
        count++
      }
    } catch {}
  }
  toast(`Refreshed ${count} calibration value(s)`)
}

async function snapshotOriginals(vars) {
  for (const v of vars) {
    const key = calKey(v)
    if (calOriginals[key] !== undefined) continue
    try {
      const r = await invoke('experiment_cal_original', { deviceId: configId(v), idx: v.idx })
      if (r.success) calOriginals[key] = r.value
    } catch {}
  }
}

function editCal(v) {
  if (!v.connected) { toast('Device not connected', true); return }
  calEditTarget.value = v
  calEditValue.value = calValues[calKey(v)] ?? ''
  calEditMsg.value = ''
  calEditErr.value = false
}

async function saveCal() {
  if (!calEditTarget.value) return
  calWriting.value = true
  calEditMsg.value = ''
  calEditErr.value = false
  const v = calEditTarget.value
  try {
    const originalVal = calValues[calKey(v)]
    const res = await invoke('cal_write', {
      deviceId: configId(v),
      idx: v.idx,
      value: calEditValue.value
    })
    if (res.success) {
      const rr = await invoke('cal_read', { deviceId: configId(v), idx: v.idx })
      if (rr.success) calValues[calKey(v)] = (rr.is_enum && rr.display_value) ? rr.display_value : rr.value
      // Track modification for DCM export
      const cpKey = `${configId(v)}:${v.name}`
      changedParams[cpKey] = {
        original_value: originalVal != null ? String(originalVal) : '',
        current_value: calEditValue.value,
      }
      calEditMsg.value = '✓ Written successfully'
      calEditErr.value = false
      setTimeout(() => { calEditTarget.value = null }, 700)
    } else {
      calEditMsg.value = res.error || 'Write failed'
      calEditErr.value = true
    }
  } catch (e) {
    calEditMsg.value = String(e)
    calEditErr.value = true
  } finally {
    calWriting.value = false
  }
}

// ── Info modal ────────────────────────────────────────────────────────────────
async function showInfo(v) {
  infoTarget.value = v
  infoData.value = null
  infoLoading.value = true
  try {
    if (v.kind === 'measurement') {
      infoData.value = {
        Name: v.name,
        Device: v.device_name,
        Type: v.type,
        Address: v.address_hex,
        Unit: v.unit || '—',
        Connected: v.connected ? 'Yes' : 'No',
      }
    } else {
      const d = await invoke('cal_detail', { deviceId: configId(v), idx: v.idx })
      infoData.value = {
        Name: d.name,
        Description: d.description || '—',
        Type: d.type,
        'Characteristic Type': d.characteristic_type,
        Address: d.address_hex,
        Unit: d.unit || '—',
        'Min / Max': `${d.min ?? '—'} / ${d.max ?? '—'}`,
      }
    }
  } catch (e) {
    infoData.value = { Error: String(e) }
  } finally {
    infoLoading.value = false
  }
}

// ── Matrix modal ──────────────────────────────────────────────────────────────
async function showMatrix(v) {
  matrixTarget.value = v
  matrixLoading.value = true
  matrixMsg.value = ''
  matrixData.value = []
  matrixTransposed.value = false
  matrixXAxisValues.value = []
  matrixYAxisValues.value = []
  matrixXAxisName.value = ''
  matrixYAxisName.value = ''
  try {
    const [readRes, detailRes] = await Promise.all([
      invoke('cal_read', { deviceId: configId(v), idx: v.idx }),
      invoke('cal_detail', { deviceId: configId(v), idx: v.idx }),
    ])
    const axes = detailRes.axes || []
    // axes[0] = X-axis (fast index, stored as matrix rows in ECU)
    // axes[1] = Y-axis (slow index, stored as matrix cols in ECU)
    matrixXAxisValues.value = axes[0]?.values ?? []
    matrixYAxisValues.value = axes[1]?.values ?? []
    matrixXAxisName.value = axes[0]?.pts_ref_name ?? 'X'
    matrixYAxisName.value = axes[1]?.pts_ref_name ?? 'Y'

    const val = readRes.value_data ?? readRes.value
    if (Array.isArray(val) && Array.isArray(val[0])) {
      // 2D map: stored as val[x_idx][y_idx], transpose for display → display[y_idx][x_idx]
      const numX = val.length
      const numY = val[0]?.length ?? 1
      matrixData.value = Array.from({ length: numY }, (_, y) =>
        Array.from({ length: numX }, (_, x) => val[x][y]))
      matrixRows.value = numY
      matrixCols.value = numX
      matrixTransposed.value = true
    } else if (Array.isArray(val)) {
      // 1D curve or val_blk: no transpose needed
      matrixCols.value = axes[0]?.num_points ?? val.length
      const cols = matrixCols.value || 1
      const rows = Math.ceil(val.length / cols)
      matrixRows.value = rows
      matrixData.value = Array.from({ length: rows }, (_, r) =>
        val.slice(r * cols, (r + 1) * cols))
    } else {
      matrixData.value = [[val]]
      matrixRows.value = 1
      matrixCols.value = 1
    }
  } catch (e) {
    matrixMsg.value = String(e)
    matrixErr.value = true
  } finally {
    matrixLoading.value = false
  }
}

async function writeCell(ri, ci, val) {
  if (!matrixTarget.value) return
  const v = matrixTarget.value
  matrixMsg.value = ''
  matrixErr.value = false
  // For transposed 2D maps: display[ri=y_idx][ci=x_idx] → ECU row=x_idx, col=y_idx
  const ecuRow = matrixTransposed.value ? ci : ri
  const ecuCol = matrixTransposed.value ? ri : ci
  try {
    const res = await invoke('cal_write_element', {
      deviceId: configId(v), idx: v.idx, row: ecuRow, col: ecuCol, value: val
    })
    if (res.success) {
      if (matrixData.value[ri]) matrixData.value[ri][ci] = val
      matrixMsg.value = `Written [${ri},${ci}] = ${val}`
      // Mark as modified (map/curve: no simple original→new, just flag it)
      const cpKey = `${configId(v)}:${v.name}`
      changedParams[cpKey] = changedParams[cpKey] || { original_value: '', current_value: '' }
    } else {
      matrixMsg.value = res.error || 'Write failed'
      matrixErr.value = true
    }
  } catch (e) {
    matrixMsg.value = String(e)
    matrixErr.value = true
  }
}

// ── Page mode ─────────────────────────────────────────────────────────────────
async function togglePageMode() {
  const newMode = pageMode.value === 'working' ? 'reference' : 'working'
  try {
    const res = await invoke('experiment_pagemode', { mode: newMode })
    if (res.success) {
      pageMode.value = newMode
      toast(`Switched to ${newMode} page`)
    } else {
      toast(`Page mode switch failed: ${res.failed} device(s)`, true)
    }
  } catch (e) { toast(String(e), true) }
}

// ── Checksum ──────────────────────────────────────────────────────────────────
async function runChecksum() {
  showChecksumModal.value = true
  checksumLoading.value = true
  checksumResults.value = []
  try {
    const res = await invoke('experiment_checksum', { config: {} })
    checksumResults.value = res.results || []
  } catch (e) {
    toast(String(e), true)
    showChecksumModal.value = false
  } finally {
    checksumLoading.value = false
  }
}

// ── Connect modal ─────────────────────────────────────────────────────────────
async function openConnectModal() {
  showConnectModal.value = true
  await refreshConnectDevices()
}

async function refreshConnectDevices() {
  try {
    const res = await invoke('list_devices')
    connectDevices.value = res.devices || []
    for (const dev of connectDevices.value) {
      if (!(dev.id in connectTransport)) {
        connectTransport[dev.id] = ''
      }
    }
  } catch (e) {
    toast('Failed to load devices: ' + String(e), true)
  }
}

async function connectDev(dev) {
  connectBusy[dev.id] = true
  try {
    const res = await invoke('connect_device', {
      deviceId: dev.id,
      transportProtocol: connectTransport[dev.id] || null,
    })
    if (res.success) {
      toast(res.message || `Connected to ${dev.name}`)
      await refreshConnectDevices()
      await loadVariables()
    } else {
      toast(res.error || 'Connect failed', true)
    }
  } catch (e) {
    toast(String(e), true)
  } finally {
    connectBusy[dev.id] = false
  }
}

async function disconnectDev(dev) {
  connectBusy[dev.id] = true
  try {
    const res = await invoke('disconnect_device', { deviceId: dev.id })
    if (res.success) {
      toast(res.message || `Disconnected from ${dev.name}`)
      await refreshConnectDevices()
      await loadVariables()
    } else {
      toast(res.error || 'Disconnect failed', true)
    }
  } catch (e) {
    toast(String(e), true)
  } finally {
    connectBusy[dev.id] = false
  }
}

async function connectAllDevs() {
  connectAllBusy.value = true
  try {
    const protocols = {}
    for (const [id, proto] of Object.entries(connectTransport)) {
      if (proto) protocols[id] = proto
    }
    const res = await invoke('connect_all_devices', {
      transportProtocols: Object.keys(protocols).length > 0 ? protocols : null,
    })
    toast(res.message || 'Done')
    await refreshConnectDevices()
    await loadVariables()
  } catch (e) {
    toast(String(e), true)
  } finally {
    connectAllBusy.value = false
  }
}

// ── Config modal ──────────────────────────────────────────────────────────────
function openConfigModal() {
  showConfigModal.value = true
}

function goToDevMgr() {
  showConfigModal.value = false
  router.push('/devices')
}

async function renameDevice(deviceId, newName) {
  if (!newName || !newName.trim()) return
  try {
    await invoke('rename_device', { deviceId, name: newName.trim() })
    await pollStatus()
  } catch (e) {
    toast('Rename failed: ' + String(e), true)
  }
}

function saveConfig() {
  // mf4Pattern and mf4Dir are reactive; just persist to localStorage
  try {
    const stored = JSON.parse(localStorage.getItem(STORAGE_KEY) || '{}')
    stored.mf4Pattern = mf4Pattern.value
    stored.mf4Dir = mf4Dir.value
    localStorage.setItem(STORAGE_KEY, JSON.stringify(stored))
  } catch {}
  showConfigModal.value = false
  toast('✓ Settings saved')
}

// ── DCM Export picker ─────────────────────────────────────────────────────────

function openDcmExportModal() {
  // Collect all calibration vars from all tabs, deduped by device_id:name
  const seen = new Set()
  const groupMap = {}

  for (const tab of tabs.value) {
    for (const v of tab.variables) {
      if (v.kind !== 'calibration') continue
      const devId = v.config_device_id || v.device_id || ''
      const key = `${devId}:${v.name}`
      if (seen.has(key)) continue
      seen.add(key)

      if (!groupMap[devId]) {
        groupMap[devId] = {
          device_id: devId,
          device_name: v.device_name || devId,
          variables: [],
        }
      }

      const isChanged = !!changedParams[key]
      let changeDetail = ''
      if (isChanged) {
        const cp = changedParams[key]
        if (cp.original_value != null && cp.current_value != null && cp.original_value !== '' && cp.current_value !== '') {
          changeDetail = `${cp.original_value} → ${cp.current_value}`
        } else {
          changeDetail = '[map/curve: elements edited]'
        }
      }

      groupMap[devId].variables.push({
        key,
        name: v.name,
        calType: v.calType || v.characteristic_type || 'VALUE',
        is_changed: isChanged,
        changeDetail,
        device_id: devId,
      })
    }
  }

  dcmVarGroups.value = Object.values(groupMap)
  // Reset selection: all checked
  for (const key in dcmSelected) delete dcmSelected[key]
  let total = 0
  for (const g of dcmVarGroups.value) {
    for (const v of g.variables) {
      dcmSelected[v.key] = true
      total++
    }
  }
  dcmTotalCount.value = total
  dcmSelectedCount.value = total
  dcmSelectAll.value = true
  showDcmModal.value = true
}

function toggleDcmAll() {
  const checked = dcmSelectAll.value
  for (const g of dcmVarGroups.value) {
    for (const v of g.variables) {
      dcmSelected[v.key] = checked
    }
  }
  updateDcmCount()
}

function updateDcmCount() {
  let count = 0
  for (const key in dcmSelected) {
    if (dcmSelected[key]) count++
  }
  dcmSelectedCount.value = count
  const total = dcmTotalCount.value
  dcmSelectAll.value = count === total
}

async function doDcmExport() {
  const selected = []
  for (const g of dcmVarGroups.value) {
    for (const v of g.variables) {
      if (dcmSelected[v.key]) {
        selected.push({ device_id: v.device_id, name: v.name })
      }
    }
  }
  if (selected.length === 0) {
    toast('No variables selected', true)
    return
  }

  const savePath = await save({
    title: 'Save DCM File',
    defaultPath: `${expTitle.value || 'export'}.dcm`,
    filters: [{ name: 'DCM Files', extensions: ['dcm'] }],
  })
  if (!savePath) return

  dcmExporting.value = true
  try {
    const res = await invoke('experiment_dcm_export', {
      config: {
        path: savePath,
        title: expTitle.value || 'export',
        variables: selected,
      },
    })
    if (res.success) {
      showDcmModal.value = false
      toast(`✓ Exported ${res.count} parameter(s) to DCM`)
    } else {
      toast(res.error || 'Export failed', true)
    }
  } catch (e) {
    toast(String(e), true)
  } finally {
    dcmExporting.value = false
  }
}

// ── Status polling ────────────────────────────────────────────────────────────
async function pollStatus() {
  try {
    const res = await invoke('experiment_status')
    statusDevices.value = res.devices || []
    if (res.recording_count > 0) mode.value = 'recording'
    else if (res.daq_running_count > 0) mode.value = 'running'
    else mode.value = 'idle'
  } catch {}
}

// ── Variable loading ──────────────────────────────────────────────────────────
async function loadVariables() {
  try {
    const res = await invoke('experiment_variables')
    allMeasurements.value = (res.measurements || []).map(m => ({
      ...m, kind: 'measurement'
    }))
    allCalibrations.value = (res.calibrations || []).map(p => ({
      ...p,
      kind: 'calibration',
      calType: p.characteristic_type || p.type || 'VALUE',
      isMapCurve: checkIsMapCurve(p),
    }))
  } catch (e) {
    toast('Failed to load variables: ' + String(e), true)
  }
}

// ── Persistence ───────────────────────────────────────────────────────────────
function saveState() {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({
      expTitle: expTitle.value,
      tabs: tabs.value,
      activeTabId: activeTabId.value,
      tabCounter: tabCounter.value,
      mf4Pattern: mf4Pattern.value,
      mf4Dir: mf4Dir.value,
    }))
  } catch {}
}

function loadState() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return false
    const s = JSON.parse(raw)
    if (s.expTitle) expTitle.value = s.expTitle
    if (s.tabCounter) tabCounter.value = s.tabCounter
    if (s.mf4Pattern) mf4Pattern.value = s.mf4Pattern
    if (s.mf4Dir) mf4Dir.value = s.mf4Dir
    if (Array.isArray(s.tabs) && s.tabs.length > 0) {
      tabs.value = s.tabs
      activeTabId.value = (s.activeTabId && tabs.value.find(t => t.id === s.activeTabId))
        ? s.activeTabId
        : tabs.value[0]?.id
      return true
    }
  } catch {}
  return false
}

watch(tabs, saveState, { deep: true })
watch(expTitle, saveState)

// ── Experiment JSON Save / Load ────────────────────────────────────────────────

/** Build a JSON-serializable snapshot of current experiment setup */
function buildExpSnapshot() {
  // Gather unique devices referenced by any variable in any tab
  const deviceMap = {}
  for (const tab of tabs.value) {
    for (const v of tab.variables) {
      const id = v.config_device_id || v.device_id || ''
      if (id && !deviceMap[id]) {
        const info = statusDevices.value.find(d => d.device_id === id)
        deviceMap[id] = { name: info?.device_name || v.device_name || id }
      }
    }
  }
  return {
    version: 1,
    title: expTitle.value,
    savedAt: new Date().toISOString(),
    devices: deviceMap,
    tabs: tabs.value.map(t => ({
      id: t.id,
      title: t.title,
      variables: t.variables.map(v => ({
        kind: v.kind,
        name: v.name,
        device_id: v.config_device_id || v.device_id || '',
        device_name: v.device_name || '',
      })),
    })),
  }
}

async function saveExpSetup() {
  if (mode.value !== 'idle') {
    toast('Stop DAQ before saving experiment setup', true)
    return
  }
  const savePath = await save({
    title: 'Save Experiment Setup',
    defaultPath: (expTitle.value || 'experiment') + '.exp.json',
    filters: [{ name: 'Experiment JSON', extensions: ['json'] }],
  })
  if (!savePath) return
  try {
    const snapshot = buildExpSnapshot()
    const res = await invoke('write_text_file', {
      path: savePath,
      content: JSON.stringify(snapshot, null, 2),
    })
    if (res.success) toast(`✓ Saved to ${savePath}`)
    else toast('Save failed', true)
  } catch (e) {
    toast(String(e), true)
  }
}

async function loadExpSetup() {
  if (mode.value !== 'idle') {
    toast('Stop DAQ before loading experiment setup', true)
    return
  }
  const filePath = await open({
    title: 'Load Experiment Setup',
    filters: [{ name: 'Experiment JSON', extensions: ['json'] }],
    multiple: false,
  })
  if (!filePath) return
  try {
    const res = await invoke('read_text_file', { path: typeof filePath === 'string' ? filePath : filePath[0] })
    if (!res.success) { toast('Failed to read file', true); return }
    const data = JSON.parse(res.content)
    if (!data.version || !data.tabs) { toast('Invalid experiment file', true); return }
    await resolveAndLoadExp(data)
  } catch (e) {
    toast('Load failed: ' + String(e), true)
  }
}

/**
 * Auto-match saved device IDs to current devices.
 * Unresolvable ones go into the mapping modal.
 */
async function resolveAndLoadExp(data) {
  // Current device list from status
  const currentDevs = statusDevices.value.length > 0
    ? statusDevices.value
    : (await invoke('list_devices').catch(() => []))

  const deviceIdMap = {}  // savedId -> currentId (null = skip)
  const unresolved = []

  for (const [savedId, info] of Object.entries(data.devices || {})) {
    // Try exact id match
    const exact = currentDevs.find(d => d.device_id === savedId)
    if (exact) { deviceIdMap[savedId] = savedId; continue }
    // Try name match
    const byName = currentDevs.find(d => d.device_name === info.name)
    if (byName) { deviceIdMap[savedId] = byName.device_id; continue }
    // Unresolved
    unresolved.push({ savedId, savedName: info.name || savedId, currentDevices: currentDevs })
  }

  if (unresolved.length > 0) {
    // Show mapping modal
    deviceMapRows.value = unresolved
    for (const r of unresolved) deviceMapSelections[r.savedId] = ''
    pendingLoadData = { data, deviceIdMap }
    showDeviceMapModal.value = true
    return
  }

  applyLoadedExp(data, deviceIdMap)
}

function applyDeviceMap() {
  if (!pendingLoadData) return
  const { data, deviceIdMap } = pendingLoadData
  for (const row of deviceMapRows.value) {
    deviceIdMap[row.savedId] = deviceMapSelections[row.savedId] || null
  }
  showDeviceMapModal.value = false
  pendingLoadData = null
  applyLoadedExp(data, deviceIdMap)
}

function cancelDeviceMap() {
  showDeviceMapModal.value = false
  pendingLoadData = null
}

/**
 * Remap device IDs, re-resolve variable indices from current allMeasurements/allCalibrations.
 */
function applyLoadedExp(data, deviceIdMap) {
  if (data.title) expTitle.value = data.title

  const newTabs = []
  for (const savedTab of data.tabs) {
    const resolvedVars = []
    for (const sv of savedTab.variables) {
      const newDevId = deviceIdMap[sv.device_id] ?? null
      if (newDevId === null) continue  // device skipped

      // Re-resolve the variable by name + device_id from live lists
      const pool = sv.kind === 'measurement' ? allMeasurements.value : allCalibrations.value
      const found = pool.find(v => v.name === sv.name && (v.config_device_id || v.device_id) === newDevId)
      if (found) {
        resolvedVars.push({ ...found })
      } else {
        // Try name-only match (fallback)
        const nameOnly = pool.find(v => v.name === sv.name)
        if (nameOnly) resolvedVars.push({ ...nameOnly })
      }
    }
    newTabs.push({ ...savedTab, variables: resolvedVars })
  }

  if (newTabs.length > 0) {
    tabs.value = newTabs
    activeTabId.value = newTabs[0].id
    tabCounter.value = Math.max(...newTabs.map(t => parseInt(t.id.replace('tab', '')) || 0)) + 1
  }
  toast(`✓ Loaded "${data.title || 'experiment'}" (${newTabs.length} tab(s))`)
}

// ── Lifecycle ─────────────────────────────────────────────────────────────────

function handleEsc(e) {
  if (e.key !== 'Escape') return
  if (showHelpModal.value) { showHelpModal.value = false; return }
  if (showDeviceMapModal.value) { showDeviceMapModal.value = false; return }
  if (showDcmModal.value) { showDcmModal.value = false; return }
  if (showConfigModal.value) { showConfigModal.value = false; return }
  if (showConnectModal.value) { showConnectModal.value = false; return }
  if (showChecksumModal.value) { showChecksumModal.value = false; return }
  if (matrixTarget.value) { matrixTarget.value = null; return }
  if (infoTarget.value) { infoTarget.value = null; return }
  if (calEditTarget.value) { calEditTarget.value = null; return }
  if (showVarModal.value) { showVarModal.value = false; return }
}

onMounted(async () => {
  const hadState = loadState()
  if (!hadState) addTab()

  await loadVariables()
  await pollStatus()

  // Snapshot originals for already-saved cal vars
  const calVars = tabs.value.flatMap(t => t.variables.filter(v => v.kind === 'calibration'))
  snapshotOriginals(calVars)

  // Listen for DAQ live data
  daqUnlisten = await listen('daq-data', event => {
    const { device_id, values } = event.payload
    if (values && typeof values === 'object') {
      for (const [name, val] of Object.entries(values)) {
        liveValues[`${device_id}:${name}`] = val
      }
    }
  })

  statusPollInterval = setInterval(pollStatus, 3000)

  // ESC key closes the topmost open modal
  window.addEventListener('keydown', handleEsc)
})

onUnmounted(() => {
  if (statusPollInterval) clearInterval(statusPollInterval)
  if (daqUnlisten) daqUnlisten()
  window.removeEventListener('keydown', handleEsc)
})
</script>

<style scoped>
/* ── Container layout ─────────────────────────────────────────────────────── */
.exp-container {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: #090909;
  color: #00cc44;
  font-family: Consolas, 'Courier New', monospace;
  overflow: hidden;
}

/* ── Title bar ────────────────────────────────────────────────────────────── */
.exp-title-bar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 16px;
  background: #060606;
  border-bottom: 1px solid #003311;
  flex-shrink: 0;
}
.exp-icon { font-size: 18px; }
.exp-title-input {
  background: transparent;
  border: none;
  border-bottom: 1px solid #002211;
  color: #00cc44;
  font-size: 16px;
  font-weight: 600;
  min-width: 180px;
  padding: 2px 4px;
  outline: none;
}
.exp-title-input:focus { border-bottom-color: #00ff66; }
.title-spacer { flex: 1; }
.state-label {
  padding: 3px 10px;
  border-radius: 12px;
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 1px;
}
.state-label.idle { background: #0a0a0a; color: #556644; }
.state-label.running { background: #003322; color: #00ff66; }
.state-label.recording { background: #330011; color: #ff4466; }

/* ── Body layout ──────────────────────────────────────────────────────────── */
.exp-body {
  display: flex;
  flex: 1;
  overflow: hidden;
}

/* ── Sidebar ──────────────────────────────────────────────────────────────── */
.exp-sidebar {
  width: 52px;
  background: #0d0d0d;
  border-right: 1px solid #003311;
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 8px 0;
  gap: 4px;
  flex-shrink: 0;
}
.sb-btn {
  width: 36px;
  height: 36px;
  background: transparent;
  border: 1px solid transparent;
  border-radius: 6px;
  color: #558844;
  font-size: 16px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.15s;
}
.sb-btn:hover { background: #003311; color: #00ff66; border-color: #003311; }
.sb-btn.rec-btn { color: #ff4466; }
.sb-btn.rec-btn:hover { background: #330011; }
.sb-btn.stop-btn { color: #ffcc00; }
.sb-btn.stop-btn:hover { background: #2a1500; }
.sb-btn.sb-help { color: #558844; font-size: 14px; }
.sb-btn.sb-help:hover { background: #0a0a0a; color: #00ff66; }
.sb-sep {
  width: 30px;
  height: 1px;
  background: #0a0a0a;
  margin: 4px 0;
}
.sb-flex { flex: 1; }
/* Page mode toggle */
.pm-toggle-wrap {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 3px;
  cursor: pointer;
  padding: 4px;
  border-radius: 6px;
  transition: background 0.15s;
}
.pm-toggle-wrap:hover { background: #003311; }
.pm-label {
  font-size: 10px;
  font-weight: 700;
  color: #335522;
  line-height: 1;
}
.pm-label.active { color: #00ff66; }
.pm-toggle {
  width: 20px;
  height: 32px;
  background: #0a0a0a;
  border-radius: 10px;
  position: relative;
  transition: background 0.2s;
}
.pm-toggle.working { background: #005522; }
.pm-thumb {
  width: 14px;
  height: 14px;
  background: #666;
  border-radius: 50%;
  position: absolute;
  left: 3px;
  top: 3px;
  transition: top 0.2s, background 0.2s;
}
.pm-toggle.working .pm-thumb { top: 15px; background: #00ff66; }

/* ── Right panel ──────────────────────────────────────────────────────────── */
.exp-right {
  display: flex;
  flex-direction: column;
  flex: 1;
  overflow: hidden;
}

/* ── Tab bar ──────────────────────────────────────────────────────────────── */
.exp-tab-bar {
  display: flex;
  align-items: stretch;
  background: #0d0d0d;
  border-bottom: 1px solid #003311;
  flex-shrink: 0;
  overflow-x: auto;
  overflow-y: hidden;
}
.exp-tab {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 14px;
  cursor: pointer;
  border-right: 1px solid #003311;
  color: #556644;
  font-size: 13px;
  white-space: nowrap;
  border-bottom: 2px solid transparent;
  transition: all 0.15s;
}
.exp-tab:hover { background: #111111; color: #00aa33; }
.exp-tab.active { background: #090909; color: #00cc44; border-bottom-color: #00ff66; }
.tab-title { cursor: text; }
.tab-close {
  color: #335522;
  font-size: 16px;
  line-height: 1;
  cursor: pointer;
  padding: 0 2px;
  border-radius: 3px;
}
.tab-close:hover { color: #ff4466; background: #1a0000; }
.tab-add-btn {
  background: transparent;
  border: none;
  color: #446633;
  font-size: 18px;
  cursor: pointer;
  padding: 6px 12px;
  align-self: center;
}
.tab-add-btn:hover { color: #00ff66; }

/* ── Tab content ──────────────────────────────────────────────────────────── */
.exp-content {
  flex: 1;
  overflow-y: auto;
  padding: 16px;
}

/* Empty state */
.exp-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 60%;
  gap: 12px;
  color: #335522;
}
.empty-icon { font-size: 48px; }
.exp-empty p { margin: 0; font-size: 15px; }
.empty-hint { font-size: 13px; color: #444; text-align: center; }

/* ── Sections ─────────────────────────────────────────────────────────────── */
.exp-section { margin-bottom: 24px; }
.section-hdr {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  font-weight: 700;
  letter-spacing: 1.2px;
  color: #556644;
  margin-bottom: 12px;
  padding-bottom: 6px;
  border-bottom: 1px solid #003311;
}
.section-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
}
.section-dot.meas { background: #00cc44; }
.section-dot.cal { background: #ffcc00; }
.section-count {
  background: #0a0a0a;
  border-radius: 10px;
  padding: 1px 7px;
  font-size: 11px;
  color: #558844;
}
.pm-indicator {
  margin-left: auto;
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 10px;
}
.pm-indicator.working { background: #003322; color: #00ff66; }
.pm-indicator.reference { background: #060606; color: #556644; }

/* ── Variable cards grid ──────────────────────────────────────────────────── */
.var-cards-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 10px;
}

.var-card {
  background: #111111;
  border-radius: 8px;
  border: 1px solid #002211;
  padding: 10px 12px;
  position: relative;
  transition: border-color 0.15s;
}
.var-card:hover { border-color: #003311; }

.meas-card { border-left: 3px solid #00cc44; }
.cal-card { border-left: 3px solid #ffcc00; }

.cal-card.editable { cursor: pointer; }
.cal-card.editable:hover { background: #2a2a3f; border-left-color: #ffcc00; }
.cal-card.readonly { cursor: default; }

.map-card { border-left: 3px solid #ffcc00; }

.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 4px;
}
.card-device { font-size: 10px; color: #446633; }
.card-actions { display: flex; gap: 4px; }
.card-info-btn, .card-remove {
  cursor: pointer;
  font-size: 12px;
  color: #335522;
  line-height: 1;
  padding: 2px 4px;
  border-radius: 3px;
}
.card-info-btn:hover { color: #00cc44; background: #001a09; }
.card-remove:hover { color: #ff4466; background: #1a0000; }

.card-name {
  font-size: 13px;
  font-weight: 600;
  color: #00cc44;
  margin-bottom: 6px;
  word-break: break-word;
}
.card-value {
  display: flex;
  align-items: baseline;
  gap: 5px;
}
.val-num {
  font-size: 18px;
  font-weight: 700;
  font-family: 'Consolas', monospace;
  color: #00ff66;
}
.val-unit { font-size: 11px; color: #446633; }

.change-mark { font-size: 11px; }
.mark-up { color: #ff4466; }
.mark-down { color: #00cc44; }

.card-type { font-size: 11px; color: #556644; margin-bottom: 8px; }
.map-actions { display: flex; gap: 6px; margin-top: 8px; }

/* ── Status bar ───────────────────────────────────────────────────────────── */
.exp-status-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 14px;
  background: #060606;
  border-top: 1px solid #003311;
  font-size: 12px;
  flex-shrink: 0;
  min-height: 28px;
}
.stat-item { display: flex; align-items: center; gap: 5px; }
.stat-item.muted { color: #335522; }
.stat-dot { font-size: 8px; }
.stat-dot.ok { color: #00ff66; }
.stat-dot.err { color: #ff4466; }
.stat-sep { color: #333; }
.stat-spacer { flex: 1; }
.daq-running { color: #00ff66; font-weight: 600; font-size: 11px; }
.daq-recording { color: #ff4466; font-weight: 600; font-size: 11px; }
.stat-toast { padding: 2px 8px; border-radius: 10px; font-size: 11px; }
.stat-toast.ok { background: #003322; color: #00ff66; }
.stat-toast.err { background: #330011; color: #ff4466; }
.stat-pm { padding: 2px 8px; border-radius: 10px; font-size: 11px; font-weight: 700; }
.stat-pm.working { background: #003322; color: #00ff66; }
.stat-pm.reference { background: #090909; color: #335522; border: 1px solid #002211; }

/* ── Buttons ──────────────────────────────────────────────────────────────── */
.exp-btn {
  padding: 6px 14px;
  border-radius: 5px;
  border: 1px solid #003311;
  background: #0a0a0a;
  color: #00aa33;
  cursor: pointer;
  font-size: 13px;
  transition: all 0.15s;
}
.exp-btn:hover { background: #111111; color: #00ff66; }
.exp-btn.primary { background: #004433; border-color: #00ff66; color: #00ff66; }
.exp-btn.primary:hover { background: #005544; }
.exp-btn.sm { padding: 3px 8px; font-size: 12px; }

/* ── Shared input/select ──────────────────────────────────────────────────── */
.exp-inp {
  padding: 7px 10px;
  background: #090909;
  border: 1px solid #003311;
  border-radius: 5px;
  color: #00cc44;
  font-size: 13px;
  outline: none;
}
.exp-inp:focus { border-color: #00ff66; }
.exp-sel {
  padding: 7px 10px;
  background: #090909;
  border: 1px solid #003311;
  border-radius: 5px;
  color: #00cc44;
  font-size: 13px;
}

/* ── Modals ───────────────────────────────────────────────────────────────── */
.modal-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.7);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}
.modal {
  background: #0d0d0d;
  border: 1px solid #002211;
  border-radius: 10px;
  display: flex;
  flex-direction: column;
  max-height: 85vh;
  min-width: 320px;
  box-shadow: 0 8px 32px rgba(0,0,0,0.6);
}
.modal.modal-xl {
  width: 820px;
  max-width: 96vw;
}
.modal.matrix-modal {
  width: 700px;
  max-width: 95vw;
}
.modal-hdr {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 18px;
  border-bottom: 1px solid #002211;
  flex-shrink: 0;
}
.modal-hdr h3 { margin: 0; font-size: 15px; color: #00cc44; }
.modal-close {
  background: transparent;
  border: none;
  color: #556644;
  font-size: 20px;
  cursor: pointer;
  padding: 0 4px;
  line-height: 1;
}
.modal-close:hover { color: #ff4466; }
.modal-filters {
  display: flex;
  gap: 8px;
  padding: 12px 18px;
  border-bottom: 1px solid #003311;
  flex-shrink: 0;
}
.modal-filters .exp-inp { flex: 1; }
.modal-body {
  flex: 1;
  overflow-y: auto;
  padding: 16px 18px;
}
.modal-footer {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 12px 18px;
  border-top: 1px solid #002211;
  flex-shrink: 0;
}
.modal-count { flex: 1; font-size: 12px; color: #556644; }
.modal-msg {
  padding: 8px 14px;
  border-radius: 5px;
  font-size: 13px;
  margin: 8px 18px 0;
}
.modal-msg-inline { font-size: 12px; flex: 1; }
.msg-ok { background: #002211; color: #00ff66; }
.msg-err { background: #220011; color: #ff4466; }

.field-row { display: flex; align-items: center; gap: 8px; }
.field-label { color: #556644; font-size: 13px; }
.bright { color: #00ff66; font-weight: 600; font-family: monospace; }

.loading { text-align: center; padding: 24px; color: #335522; }
.loading.muted { color: #444; }

/* ── Variable selection table ─────────────────────────────────────────────── */
.var-table { width: 100%; border-collapse: collapse; font-size: 13px; }
.var-table th {
  padding: 8px 10px;
  border-bottom: 1px solid #002211;
  text-align: left;
  color: #556644;
  font-weight: 600;
  font-size: 11px;
  letter-spacing: 0.5px;
  background: #0a0a0a;
  position: sticky;
  top: 0;
}
.var-table td { padding: 7px 10px; border-bottom: 1px solid #001a09; }
.var-table tr:hover td { background: #111111; }
.var-table tr.selected td { background: #001a09; }
.var-table tr { cursor: pointer; }
.vt-name { font-weight: 600; }
.vt-dev { color: #556644; }
.vt-kind { font-size: 12px; }
.vt-type { color: #556644; font-size: 12px; }
.vt-addr { font-family: monospace; font-size: 12px; color: #558844; }
.mono { font-family: 'Consolas', monospace; }

/* ── Info table ───────────────────────────────────────────────────────────── */
.info-table { width: 100%; border-collapse: collapse; font-size: 13px; }
.info-table td { padding: 8px 10px; border-bottom: 1px solid #001a09; }
.info-key { color: #556644; font-weight: 600; width: 40%; }

/* ── Matrix table ─────────────────────────────────────────────────────────── */
.matrix-meta { font-size: 12px; color: #556644; margin-top: 4px; }
.axis-label { color: #00cc44; }
.matrix-scroll { overflow-x: auto; }
.matrix-table { border-collapse: collapse; }
.matrix-corner {
  padding: 4px 8px;
  color: #00cc44;
  font-size: 11px;
  font-weight: 600;
  background: #060606;
  border-right: 1px solid #002211;
  border-bottom: 1px solid #002211;
  text-align: right;
}
.matrix-ri {
  padding: 4px 8px;
  color: #00cc44;
  font-size: 11px;
  font-weight: 600;
  border-right: 1px solid #002211;
  user-select: none;
  background: #060606;
  text-align: right;
  min-width: 52px;
}
.matrix-col-hdr {
  padding: 4px 6px;
  color: #00cc44;
  font-size: 11px;
  text-align: center;
  background: #060606;
  border-bottom: 1px solid #002211;
  font-weight: 600;
}
.matrix-cell {
  width: 72px;
  padding: 4px 6px;
  background: #090909;
  border: 1px solid #002211;
  border-radius: 3px;
  color: #00cc44;
  font-family: monospace;
  font-size: 12px;
  text-align: right;
}
.matrix-cell:disabled { color: #446633; }
.matrix-cell:focus { outline: none; border-color: #00ff66; }
.matrix-table td { padding: 2px; }

/* ── Checksum results ─────────────────────────────────────────────────────── */
.chk-device { margin-bottom: 16px; }
.chk-dev-hdr {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 14px;
  border-radius: 5px 5px 0 0;
  font-weight: 600;
}
.chk-dev-hdr.pass { background: #002211; color: #00ff66; }
.chk-dev-hdr.fail { background: #220011; color: #ff4466; }
.chk-status { font-size: 12px; }
.chk-error { padding: 8px 14px; color: #ff4466; font-size: 13px; background: #1a0000; }
.chk-table { width: 100%; border-collapse: collapse; font-size: 13px; }
.chk-table th { padding: 6px 10px; background: #0a0a0a; color: #556644; text-align: left; }
.chk-table td { padding: 6px 10px; border-bottom: 1px solid #001a09; }
.chk-table tr.ok-row td { color: #558844; }
.chk-table tr.fail-row td { color: #ff8888; }
.chk-match { font-weight: 700; }

/* ── Connect modal ────────────────────────────────────────────────────────── */
.conn-device-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 0;
  border-bottom: 1px solid #001a09;
}
.conn-device-row:last-child { border-bottom: none; }
.conn-device-info {
  display: flex;
  align-items: center;
  gap: 10px;
  flex: 1;
  min-width: 0;
}
.conn-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  flex-shrink: 0;
}
.conn-dot.on { background: #00ff66; box-shadow: 0 0 6px #00ff66; }
.conn-dot.off { background: #555; }
.conn-device-text { min-width: 0; }
.conn-device-name { font-weight: 600; font-size: 14px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.conn-device-addr { font-size: 11px; color: #556644; }
.conn-device-status { flex-shrink: 0; }
.conn-device-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}
.transport-select {
  background: #0d0d0d;
  border: 1px solid #003311;
  color: #00cc44;
  border-radius: 4px;
  padding: 4px 8px;
  font-size: 12px;
  cursor: pointer;
}
.tag {
  padding: 2px 8px;
  border-radius: 10px;
  font-size: 11px;
  font-weight: 600;
}
.tag.connected { background: #003322; color: #00ff66; }
.tag.disconnected { background: #0a0a0a; color: #556644; }

/* ── Config Modal ─────────────────────────────────────────────────────────── */
.form-group { margin-bottom: 14px; }
.form-label { display: block; font-size: 12px; color: #556644; margin-bottom: 5px; text-transform: uppercase; letter-spacing: 0.05em; }
.form-input { width: 100%; background: #060606; border: 1px solid #002211; border-radius: 4px; padding: 7px 10px; color: #00aa33; font-size: 13px; box-sizing: border-box; }
.form-input:focus { outline: none; border-color: #00cc44; }
.form-input.mono { font-family: monospace; }
.form-hint { font-size: 11px; color: #335522; margin-top: 4px; }
.cfg-dev-list { display: flex; flex-direction: column; gap: 6px; max-height: 180px; overflow-y: auto; }
.cfg-dev-row { display: flex; align-items: center; gap: 8px; }
.cfg-dev-name { flex: 1; }
.cfg-dev-link { margin-top: 10px; font-size: 13px; }
.cfg-dev-link a { color: #00aa33; text-decoration: none; }
.cfg-dev-link a:hover { text-decoration: underline; }

/* ── DCM Export Picker ────────────────────────────────────────────────────── */
.dcm-sel-bar { display: flex; align-items: center; justify-content: space-between; padding-bottom: 8px; margin-bottom: 8px; border-bottom: 1px solid #002211; }
.dcm-group { margin-bottom: 12px; }
.dcm-group-hdr { font-size: 11px; color: #556644; text-transform: uppercase; letter-spacing: 0.05em; border-bottom: 1px solid #2a2a2a; padding-bottom: 3px; margin-bottom: 6px; }
.dcm-var-row { display: flex; align-items: flex-start; gap: 8px; padding: 4px 6px; border-radius: 3px; cursor: pointer; transition: background 0.1s; font-size: 13px; }
.dcm-var-row:hover { background: #090909; }
.dcm-var-row input[type="checkbox"] { margin-top: 2px; flex-shrink: 0; }
.dcm-var-info { flex: 1; min-width: 0; }
.dcm-var-name { color: #00ff66; font-family: monospace; font-size: 13px; }
.dcm-var-type { color: #446633; font-size: 11px; margin-left: 6px; }
.dcm-modified-badge { color: #ffcc00; font-size: 11px; font-weight: 700; margin-left: 6px; }
.dcm-change-detail { color: #558844; font-size: 11px; margin-top: 2px; }
.devmap-row {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 10px 0;
  border-bottom: 1px solid #001a09;
}
.devmap-row:last-child { border-bottom: none; }
.devmap-saved { flex: 1; min-width: 0; }
.devmap-name { font-size: 14px; color: #00aa33; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.devmap-id { font-size: 11px; color: #335522; font-family: monospace; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.devmap-arrow { color: #335522; flex-shrink: 0; font-size: 18px; }
.devmap-sel { flex: 1; min-width: 160px; }

/* ── Help Modal ───────────────────────────────────────────────────────────── */
.help-section { margin-bottom: 18px; }
.help-h4 { font-size: 14px; color: #558844; margin: 0 0 8px; font-weight: 600; }
.help-h4.green { color: #00cc44; }
.help-ol, .help-ul { margin: 0 0 0 18px; padding: 0; color: #00aa33; font-size: 13px; line-height: 1.7; }
.help-table { width: 100%; border-collapse: collapse; font-size: 13px; color: #00aa33; }
.help-table td { padding: 5px 10px; border-bottom: 1px solid #001a09; vertical-align: top; }
.help-table tr:last-child td { border-bottom: none; }
.help-btn { width: 32px; text-align: center; font-size: 16px; color: #558844; }
.help-states { display: flex; align-items: center; gap: 10px; margin-top: 6px; flex-wrap: wrap; }
.help-state { padding: 8px 14px; border-radius: 6px; font-size: 13px; text-align: center; line-height: 1.4; }
.help-state.idle { background: #0a0a0a; color: #556644; border: 1px solid #002211; }
.help-state.running { background: #002200; color: #00cc44; border: 1px solid #00aa44; }
.help-state.recording { background: #200010; color: #ff4466; border: 1px solid #880022; }
.help-arrow { color: #444; font-size: 20px; }
</style>

