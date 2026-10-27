<script lang="ts">
  import { onMount } from 'svelte';
import { invoke, isTauri } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { open } from '@tauri-apps/plugin-dialog';

  type Json = Record<string, any>;
  type Phase = 'ready' | 'reviewing' | 'planned' | 'running' | 'done' | 'error';

  let cliPath = $state(localStorage.getItem('cliPath') ?? '');
  let inputs = $state<string[]>([]);
  let keys = $state<string[]>(loadKeys());
  let outputDir = $state(localStorage.getItem('outputDir') ?? '');
  let tree = $state(true);
  let resume = $state(false);
  let force = $state(false);
  let onlyExefs = $state(false);
  let onlyRomfs = $state(false);
  let version = $state('');
  let phase = $state<Phase>('ready');
  let plan = $state<Json | null>(null);
  let result = $state<Json | null>(null);
  let keyStatus = $state<Json | null>(null);
  let errorMessage = $state('');
  let logs = $state<string[]>([]);
  let busy = $state(false);
  let activeTab = $state<'extract' | 'settings'>('extract');
  let unlistenLog: UnlistenFn | undefined;
  let unlistenState: UnlistenFn | undefined;
  let unlistenDrop: UnlistenFn | undefined;

  onMount(() => {
    if (!isTauri()) return;
    void (async () => {
      if (!cliPath) {
        cliPath = (await invoke<string | null>('default_cli_path')) ?? '';
        if (cliPath) localStorage.setItem('cliPath', cliPath);
      }
      unlistenLog = await listen<string>('dump-log', (event) => {
        logs = [...logs.slice(-99), event.payload];
      });
      unlistenState = await listen<string>('dump-state', (event) => {
        if (event.payload === 'running') phase = 'running';
        if (event.payload === 'done') phase = 'done';
        if (event.payload === 'failed') phase = 'error';
      });
      unlistenDrop = await getCurrentWebviewWindow().onDragDropEvent((event) => {
        if (event.payload.type === 'drop') {
          const dropped = event.payload.paths.filter((path) => /\.(nsp|xci)$/i.test(path));
          if (dropped.length) {
            inputs = dropped.slice(0, 2);
            invalidatePlan();
          }
        }
      });
    })();
    return () => {
      unlistenLog?.();
      unlistenState?.();
      unlistenDrop?.();
    };
  });

  function loadKeys(): string[] {
    try { return JSON.parse(localStorage.getItem('keys') ?? '[]') as string[]; }
    catch { return []; }
  }

  function invalidatePlan() {
    plan = null;
    if (phase === 'planned') phase = 'ready';
  }

  const options = () => ({
    outputDir,
    tree,
    resume,
    force,
    exefsOnly: onlyExefs,
    romfsOnly: onlyRomfs,
    version: version || null
  });

  async function chooseInputs() {
    const selected = await open({
      multiple: true,
      filters: [{ name: 'Contenedores Nintendo Switch', extensions: ['nsp', 'xci'] }]
    });
    if (selected) {
      inputs = Array.isArray(selected) ? selected : [selected];
      invalidatePlan();
      result = null;
      phase = 'ready';
    }
  }

  async function chooseKeys() {
    const selected = await open({
      multiple: true,
      filters: [{ name: 'Archivos de claves', extensions: ['keys'] }]
    });
    if (selected) {
      keys = Array.isArray(selected) ? selected : [selected];
      localStorage.setItem('keys', JSON.stringify(keys));
      keyStatus = null;
      invalidatePlan();
    }
  }

  async function chooseOutput() {
    const selected = await open({ directory: true, multiple: false, title: 'Elegir carpeta de salida' });
    if (typeof selected === 'string') {
      outputDir = selected;
      localStorage.setItem('outputDir', outputDir);
      invalidatePlan();
    }
  }

  async function chooseCli() {
    const selected = await open({ multiple: false, title: 'Seleccionar ejecutable nsromexefs' });
    if (typeof selected === 'string') {
      cliPath = selected;
      localStorage.setItem('cliPath', cliPath);
      invalidatePlan();
    }
  }

  async function validateKeys() {
    await action(async () => {
      keyStatus = await invoke<Json>('validate_keys', { cliPath, keys });
    }, 'No se pudieron validar las claves. Revisa las rutas y el formato.');
  }

  async function reviewPlan() {
    await action(async () => {
      phase = 'reviewing';
      plan = await invoke<Json>('plan_dump', { cliPath, inputs, keys, options: options() });
      phase = 'planned';
      result = null;
    }, 'No se pudo preparar el plan. Comprueba los archivos, claves y destino.');
  }

  async function startExtraction() {
    logs = [];
    result = null;
    await action(async () => {
      result = await invoke<Json>('run_dump', { cliPath, inputs, keys, options: options() });
      phase = 'done';
    }, 'La extracción no terminó correctamente. Puedes revisar el log y reanudarla.');
  }

  async function cancelExtraction() {
    try {
      await invoke('cancel_dump');
      phase = 'error';
      errorMessage = 'Extracción cancelada. El estado queda disponible para reanudar.';
    } catch (error) {
      errorMessage = String(error);
    }
  }

  async function action(work: () => Promise<void>, fallback: string) {
    busy = true;
    errorMessage = '';
    try {
      if (!cliPath) throw new Error('Selecciona el ejecutable nsromexefs en Ajustes.');
      await work();
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : String(error || fallback);
      phase = 'error';
    } finally {
      busy = false;
    }
  }

  function humanBytes(value?: number) {
    if (value == null || !Number.isFinite(value)) return '—';
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    let size = value;
    let unit = 0;
    while (size >= 1024 && unit < units.length - 1) { size /= 1024; unit++; }
    return `${size.toLocaleString('es-ES', { maximumFractionDigits: 2 })} ${units[unit]}`;
  }

  function report(value: Json | null) { return value?.titles?.[0] ?? null; }
  function totalBytes(value: Json | null) {
    const item = report(value);
    return (item?.exefs?.bytes ?? 0) + (item?.romfs?.bytes ?? 0);
  }
</script>

<svelte:head>
  <title>nsromexefs · extracción</title>
  <meta name="description" content="Inspecciona y extrae exefs y romfs desde NSP y XCI." />
</svelte:head>

<div class="shell">
  <aside class="rail">
    <div class="brand-mark">n<span>.</span></div>
    <button class:active={activeTab === 'extract'} class="rail-button" title="Extracción" onclick={() => activeTab = 'extract'}>
      <span class="nav-icon">⇩</span><small>Extraer</small>
    </button>
    <button class:active={activeTab === 'settings'} class="rail-button" title="Ajustes" onclick={() => activeTab = 'settings'}>
      <span class="nav-icon">⚙</span><small>Ajustes</small>
    </button>
    <div class="rail-bottom"><span class="online-dot"></span><small>CLI v0.1.0</small></div>
  </aside>

  <main class="main-view">
    <header class="topbar">
      <div class="crumb"><span>NSROMEXEFS</span><b>/</b>{activeTab === 'extract' ? 'EXTRACCIÓN' : 'PREFERENCIAS'}</div>
      <div class="top-status"><span class:ok={!!cliPath} class="status-pip"></span>{cliPath ? 'Motor conectado' : 'Configura el motor CLI'}</div>
    </header>

    {#if activeTab === 'extract'}
      <section class="page-heading">
        <div>
          <div class="eyebrow">HERRAMIENTA DE EXTRACCIÓN</div>
          <h1>Prepara tu contenido<span class="heading-period">.</span></h1>
          <p>Selecciona tus archivos, revisa el plan y extrae con seguridad.</p>
        </div>
        <div class="stepper" aria-label="Progreso del flujo">
          <div class:current={phase === 'ready' || phase === 'reviewing'} class:complete={!!plan}><i>01</i><span>Preparar</span></div>
          <div class="step-line"></div>
          <div class:current={phase === 'planned'} class:complete={phase === 'running' || phase === 'done'}><i>02</i><span>Revisar</span></div>
          <div class="step-line"></div>
          <div class:current={phase === 'running' || phase === 'done'}><i>03</i><span>Extraer</span></div>
        </div>
      </section>

      <div class="content-grid">
        <section class="workflow-column">
          <article class="card source-card">
            <div class="card-heading">
              <div class="section-number">01</div>
              <div><h2>Archivos de origen</h2><p>Elige un NSP/XCI, o base y update.</p></div>
            </div>
            <button class="drop-zone" onclick={chooseInputs} disabled={busy}>
              <span class="upload-icon">↑</span>
              <span class="drop-title">Arrastra o selecciona tus archivos</span>
              <span class="drop-subtitle">Compatible con .NSP y .XCI · máximo 2 archivos</span>
              <span class="select-button">Seleccionar archivos <b>↗</b></span>
            </button>
            {#if inputs.length}
              <div class="file-list">
                {#each inputs as input, index}
                  <div class="file-row"><div class="file-badge">{input.toLowerCase().endsWith('.xci') ? 'XCI' : 'NSP'}</div><div class="file-name"><strong>{input.split(/[\\/]/).pop()}</strong><span>{input}</span></div><button class="remove-file" aria-label="Quitar archivo" onclick={() => { inputs = inputs.filter((_, i) => i !== index); plan = null; }}>×</button></div>
                {/each}
              </div>
            {/if}
            <div class="card-divider"></div>
            <div class="inline-setting">
              <div class="setting-icon key-icon">⌑</div>
              <div class="setting-copy"><strong>Archivos de claves</strong><span>{keys.length ? `${keys.length} archivo(s) seleccionado(s)` : 'prod.keys y title.keys (opcional)'}</span></div>
              <button class="text-button" onclick={chooseKeys} disabled={busy}>{keys.length ? 'Cambiar' : 'Añadir claves'} <b>＋</b></button>
            </div>
            {#if keys.length}
              <div class="key-list">{#each keys as key}<span class="key-chip">▣ {key.split(/[\\/]/).pop()}</span>{/each}<button class="mini-check" onclick={validateKeys} disabled={busy}>Validar</button></div>
            {/if}
            {#if keyStatus}
              <div class="key-result"><span class="status-pip ok"></span>Keyset válido · revisión {keyStatus.max_revision ?? '—'} · {keyStatus.titlekeys ?? 0} titlekeys</div>
            {/if}
          </article>

          <article class="card destination-card">
            <div class="card-heading">
              <div class="section-number">02</div>
              <div><h2>Destino y opciones</h2><p>Define dónde guardar los archivos extraídos.</p></div>
            </div>
            <div class="destination-row">
              <label for="destination">CARPETA DE SALIDA</label>
              <div class="path-control"><input id="destination" bind:value={outputDir} placeholder="Selecciona una carpeta…" oninput={() => { localStorage.setItem('outputDir', outputDir); invalidatePlan(); }} /><button onclick={chooseOutput} aria-label="Buscar carpeta">↗</button></div>
            </div>
            <div class="options-grid">
              <label class="check-option"><input type="checkbox" bind:checked={tree} onchange={invalidatePlan} /><span class="checkmark"></span><span><strong>Organizar por título</strong><small>Carpetas por versión y DLC</small></span></label>
              <label class="check-option"><input type="checkbox" bind:checked={resume} onchange={invalidatePlan} /><span class="checkmark"></span><span><strong>Reanudar extracción</strong><small>Verifica archivos ya escritos</small></span></label>
              <label class="check-option"><input type="checkbox" bind:checked={onlyExefs} onchange={() => { if (onlyExefs) onlyRomfs = false; invalidatePlan(); }} /><span class="checkmark"></span><span><strong>Solo exefs</strong><small>Ejecutables y metadatos</small></span></label>
              <label class="check-option"><input type="checkbox" bind:checked={onlyRomfs} onchange={() => { if (onlyRomfs) onlyExefs = false; invalidatePlan(); }} /><span class="checkmark"></span><span><strong>Solo romfs</strong><small>Datos del juego</small></span></label>
              <label class="check-option"><input type="checkbox" bind:checked={force} onchange={invalidatePlan} /><span class="checkmark"></span><span><strong>Reemplazar existentes</strong><small>Sobrescribe archivos ya extraídos</small></span></label>
            </div>
            <div class="destination-row version-row"><label for="version">VERSIÓN (OPCIONAL)</label><input id="version" bind:value={version} placeholder="Automática · ej. 1.1.0" oninput={invalidatePlan} /></div>
          </article>

          {#if errorMessage}<div class="alert-error"><span>!</span><div><strong>No se pudo completar la acción</strong><p>{errorMessage}</p></div><button onclick={() => errorMessage = ''}>×</button></div>{/if}

          <div class="action-row">
            {#if phase === 'running'}
              <button class="secondary-button cancel-button" onclick={cancelExtraction}>Cancelar extracción</button>
            {:else}
              <button class="secondary-button" onclick={reviewPlan} disabled={busy || !inputs.length || !outputDir}>{busy && phase === 'reviewing' ? 'Analizando…' : 'Revisar plan'} <span>→</span></button>
              <button class="primary-button" onclick={startExtraction} disabled={busy || !plan || (phase !== 'planned' && phase !== 'error')}>
                {busy ? 'Preparando…' : phase === 'done' ? 'Extraer de nuevo' : 'Iniciar extracción'} <span>↗</span>
              </button>
            {/if}
          </div>
        </section>

        <aside class="summary-column">
          <article class="summary-card">
            <div class="summary-top"><span class="summary-label">RESUMEN DEL PLAN</span><span class="plan-status" class:ready={!!plan}>{plan ? 'LISTO' : 'PENDIENTE'}</span></div>
            {#if report(plan)}
              {@const item = report(plan)}
              <h3>{item.display_version ? `Versión ${item.display_version}` : 'Extracción preparada'}</h3>
              <p class="title-id">Título {item.title_id}</p>
              <div class="summary-stats"><div><strong>{item.exefs?.files ?? '—'}</strong><span>archivos exefs</span></div><div><strong>{item.romfs?.files ?? '—'}</strong><span>archivos romfs</span></div></div>
              <div class="summary-total"><span>Datos estimados</span><strong>{humanBytes(totalBytes(plan))}</strong></div>
              {#if item.merged}<div class="merge-note"><span>⤳</span> Update BKTR · se fusionará con la base</div>{/if}
            {:else}
              <div class="empty-summary"><div class="empty-graphic"><span>⌁</span><i></i></div><strong>Tu plan aparecerá aquí</strong><p>Selecciona los archivos y pulsa “Revisar plan” para ver títulos, versiones y espacio requerido.</p></div>
            {/if}
            {#if phase === 'planned'}<div class="plan-confirm"><span>✓</span> Destino y espacio revisados</div>{/if}
          </article>

          {#if phase === 'running' || phase === 'done' || phase === 'error'}
            <article class="progress-card">
              <div class="progress-title"><div><span class="summary-label">ESTADO</span><h3>{phase === 'running' ? 'Extrayendo…' : phase === 'done' ? 'Extracción lista' : 'Revisa el resultado'}</h3></div><span class:spinning={phase === 'running'} class="progress-symbol">{phase === 'done' ? '✓' : phase === 'running' ? '↻' : '!'}</span></div>
              {#if phase === 'running'}<div class="progress-track"><span></span></div><p class="progress-hint">El proceso puede tardar varios minutos según el tamaño del RomFS.</p>{/if}
              {#if result}<div class="result-details"><div><span>Archivos con checksum</span><b>{report(result)?.checksums ?? '—'}</b></div><div><span>Tiempo total</span><b>{report(result)?.timings?.total_ms ? `${(report(result).timings.total_ms / 1000).toFixed(1)} s` : '—'}</b></div><div><span>Manifest</span><b>{report(result)?.manifest ? 'Generado' : '—'}</b></div></div>{/if}
              {#if logs.length}<div class="log-box">{#each logs as line}<div>{line}</div>{/each}</div>{/if}
            </article>
          {/if}

          <div class="privacy-note"><span>◈</span><p><strong>Privado por diseño</strong><br />Todo el procesamiento ocurre localmente. Tus archivos y claves no se envían a ningún servidor.</p></div>
        </aside>
      </div>
    {:else}
      <section class="page-heading settings-heading"><div><div class="eyebrow">PREFERENCIAS LOCALES</div><h1>Ajustes<span class="heading-period">.</span></h1><p>Configura la CLI que ejecuta el motor y tus archivos de claves.</p></div></section>
      <section class="settings-layout">
        <article class="card settings-card"><div class="card-heading"><div class="section-number">01</div><div><h2>Motor nsromexefs</h2><p>La GUI usa la misma CLI para mantener idéntica lógica de extracción.</p></div></div>
          <div class="destination-row"><label for="cli">EJECUTABLE CLI</label><div class="path-control"><input id="cli" bind:value={cliPath} placeholder="Ruta a nsromexefs.exe" onchange={() => { localStorage.setItem('cliPath', cliPath); invalidatePlan(); }} /><button onclick={chooseCli} aria-label="Buscar ejecutable">↗</button></div></div>
          <div class="settings-hint">Compila `nsromexefs` con Cargo y selecciona su ejecutable. La GUI todavía no se distribuye en un instalador combinado con la CLI.</div>
        </article>
        <article class="card settings-card"><div class="card-heading"><div class="section-number">02</div><div><h2>Archivos de claves</h2><p>Las claves se usan localmente y nunca se imprimen ni se suben.</p></div></div>
          <div class="settings-key-row"><div class="setting-copy"><strong>{keys.length ? `${keys.length} archivos configurados` : 'No hay archivos configurados'}</strong><span>{keys.map((key) => key.split(/[\\/]/).pop()).join(' · ') || 'Puedes cambiar la selección cuando quieras.'}</span></div><button class="text-button" onclick={chooseKeys}>Seleccionar <b>↗</b></button></div>
          <button class="secondary-button validate-button" onclick={validateKeys} disabled={!keys.length || busy}>Validar keyset <span>→</span></button>
          {#if keyStatus}<div class="key-result settings-key-result"><span class="status-pip ok"></span>Keyset válido · {keyStatus.keys} claves · revisión {keyStatus.max_revision ?? '—'}</div>{/if}
        </article>
        <article class="about-card"><div class="brand-mark small-mark">n<span>.</span></div><div><strong>nsromexefs GUI · en desarrollo</strong><p>Interfaz Tauri 2 · procesamiento local con la CLI MIT.</p><a href="https://github.com/SirEdmundiumRockwell/nsromexefs/releases" target="_blank" rel="noreferrer">Código fuente y créditos <span>↗</span></a></div></article>
      </section>
    {/if}
    <footer><span>NSROMEXEFS <i>·</i> EXTRACTION TOOL</span><span>Hecho para procesar localmente <b>◈</b></span></footer>
  </main>
</div>
