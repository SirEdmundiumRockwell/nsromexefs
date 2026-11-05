<script lang="ts">
  import { onMount } from 'svelte';
import { invoke, isTauri } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { open } from '@tauri-apps/plugin-dialog';

  type Json = Record<string, any>;
  type Phase = 'ready' | 'reviewing' | 'planned' | 'running' | 'done' | 'error';
  type EngineInfo = { path: string; version: string | null; bundled: boolean; overridden: boolean };

  let language = $state<'en' | 'es'>(localStorage.getItem('language') === 'es' ? 'es' : 'en');
  const tr = (es: string, en: string) => language === 'en' ? en : es;
  type ThemeName = 'classic' | 'ocean' | 'forest' | 'midnight';
  type FontName = 'rounded' | 'system' | 'serif' | 'mono';
  function loadTheme(): ThemeName {
    const value = localStorage.getItem('theme');
    if (value === 'nintendo') return 'classic';
    return value === 'ocean' || value === 'forest' || value === 'midnight' ? value : 'classic';
  }
  let theme = $state<ThemeName>(loadTheme());
  let font = $state<FontName>((localStorage.getItem('font') as FontName) || 'rounded');
  type EventLevel = 'info' | 'success' | 'warning' | 'error';
  type EventRecord = { at: string; level: EventLevel; es: string; en: string };
  const EVENT_LOG_KEY = 'eventLog';
  const EVENT_LOG_LIMIT = 250;
  function loadEvents(): EventRecord[] {
    try {
      const value = JSON.parse(localStorage.getItem(EVENT_LOG_KEY) ?? '[]');
      return Array.isArray(value) ? value.slice(-EVENT_LOG_LIMIT) as EventRecord[] : [];
    } catch { return []; }
  }
  let eventLog = $state<EventRecord[]>(loadEvents());
  function setLanguage(value: 'en' | 'es') {
    language = value;
    localStorage.setItem('language', value);
    document.documentElement.lang = value;
  }
  function applyAppearance() {
    document.body.dataset.theme = theme;
    document.body.dataset.font = font;
  }
  function setTheme(value: ThemeName) {
    theme = value;
    localStorage.setItem('theme', value);
    applyAppearance();
  }
  function setFont(value: FontName) {
    font = value;
    localStorage.setItem('font', value);
    applyAppearance();
  }
  function redactEventText(value: string) {
    return value
      .replace(/[A-Za-z]:\\Users\\[^\\\s]+/g, '[user path]')
      .replace(/(?:\/Users\/|\/home\/)[^/\s]+/g, '[user path]')
      .replace(/[^\s"'<>|]+\.(?:keys|pem|p12)\b/gi, '[sensitive file]');
  }
  function recordEvent(level: EventLevel, es: string, en: string) {
    const entry = { at: new Date().toISOString(), level, es: redactEventText(es), en: redactEventText(en) };
    eventLog = [...eventLog, entry].slice(-EVENT_LOG_LIMIT);
    try { localStorage.setItem(EVENT_LOG_KEY, JSON.stringify(eventLog)); } catch { /* keep the in-memory log if storage is full */ }
  }
  function clearEventLog() {
    if (!window.confirm(tr('¿Borrar todo el registro de eventos?', 'Clear the entire event log?'))) return;
    eventLog = [];
    localStorage.removeItem(EVENT_LOG_KEY);
  }
  function downloadEventLog() {
    const contents = eventLog.map((event) => `${event.at}\t${event.level.toUpperCase()}\t${language === 'en' ? event.en : event.es}`).join('\n');
    const blob = new Blob([contents], { type: 'text/plain;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = `nsromexefs-events-${new Date().toISOString().slice(0, 10)}.log`;
    link.click();
    URL.revokeObjectURL(url);
  }

  function localizeLogLine(line: string) {
    if (language === 'es') return line;
    return line.replace(/^(\s*…\s*)(.+?) de (.+)$/u, '$1$2 of $3');
  }

  function localizeError(value: unknown): string {
    const message = value instanceof Error ? value.message : String(value ?? '');
    if (language === 'es') return message;
    const rules: Array<[RegExp, string]> = [
      [/^falta la base:.*$/i, 'Missing base game: an update requires its matching base NSP/XCI container.'],
      [/^falta `header_key`.*$/i, 'The keyset is missing `header_key` (32 bytes are required).'],
      [/^falta `titlekek_(\d+)` \(generación de clave declarada: (\d+)\)$/i, 'The keyset is missing `titlekek_$1` (declared key generation: $2).'],
      [/^falta el titlekey del rights id (.+)$/i, 'The title key for rights ID $1 is missing.'],
      [/^falta `key_area_key_(.+)` en el keyset$/i, 'The keyset is missing `key_area_key_$1`.'],
      [/^ticket personalizado:.*$/i, 'Personalized ticket: its title key is encrypted with the issuing console’s RSA key.'],
      [/^archivo de claves inválido \((.+)\): (.+)$/i, 'Invalid key file ($1): $2'],
      [/^archivo de claves inválido (.+)$/i, 'Invalid key file: $1'],
      [/^ya existe (.+); usa --force para sobrescribir$/i, '$1 already exists; use --force to overwrite it.'],
      [/^espacio insuficiente en (.+): se necesitan (\d+) bytes y hay (\d+)$/i, 'Not enough disk space at $1: $2 bytes required, $3 available.'],
      [/^espacio insuficiente en (.+)$/i, 'Not enough disk space: $1'],
      [/^la verificación falló: (\d+) problema\(s\) en (\d+) archivo\(s\)$/i, 'Verification failed: $1 issue(s) across $2 file(s).'],
      [/^la verificación falló: (.+)$/i, 'Verification failed: $1'],
      [/^no está disponible la versión (.+) \(disponibles: (.+)\)$/i, 'Version $1 is not available (available: $2).'],
      [/^no está disponible la versión (.+)$/i, 'Version is unavailable: $1'],
      [/^colisión de nombres en el destino: (.+)$/i, 'Name collision in the destination: $1'],
      [/^la base no corresponde al update: el update pertenece a (.+), pero la base es (.+)$/i, 'The base game does not match the update: the update belongs to $1, but the base is $2.'],
      [/^la base no corresponde al update: (.+)$/i, 'The base game does not match the update: $1'],
      [/^el lote terminó con (\d+) error\(es\) de (\d+) título\(s\)$/i, 'Batch finished with $1 error(s) out of $2 title(s).'],
      [/^el lote terminó con (.+)$/i, 'Batch finished with errors: $1'],
      [/^formato no soportado: (.+)$/i, 'Unsupported format: $1'],
      [/^archivo inválido: (.+)$/i, 'Invalid file: $1'],
      [/^error de E\/S: (.+)$/i, 'I/O error: $1'],
      [/^Selecciona al menos un archivo de claves\.$/, 'Select at least one key file.'],
      [/^Selecciona una carpeta de destino\.$/, 'Choose an output folder.'],
      [/^Selecciona uno o dos contenedores NSP\/XCI\.$/, 'Select one or two NSP/XCI containers.'],
      [/^No encuentro el archivo: (.+)$/i, 'File not found: $1'],
      [/^No encuentro nsromexefs\..*$/, 'Could not find nsromexefs. Select the CLI executable in Settings.'],
      [/^No se pudo ejecutar nsromexefs: (.+)$/i, 'Could not run nsromexefs: $1'],
      [/^No se pudo iniciar nsromexefs: (.+)$/i, 'Could not start nsromexefs: $1'],
      [/^No se pudo cancelar la extracción: (.+)$/i, 'Could not cancel extraction: $1'],
      [/^No se pudo consultar el proceso: (.+)$/i, 'Could not check the process: $1'],
      [/^Falló la lectura de la respuesta de nsromexefs\.$/i, 'Could not read the response from nsromexefs.'],
      [/^El estado de extracción no está disponible\.$/i, 'Extraction state is unavailable.'],
      [/^Ya hay una extracción en curso\.$/i, 'An extraction is already running.'],
      [/^No hay (?:un proceso de extracción activo|ninguna extracción en curso)\.$/i, 'There is no active extraction process.'],
      [/^La extracción se canceló\.$/i, 'The extraction was cancelled.'],
      [/^nsromexefs terminó con código (.+)\.$/i, 'nsromexefs exited with code $1.'],
      [/^nsromexefs devolvió JSON no válido: (.+)$/i, 'nsromexefs returned invalid JSON: $1'],
    ];
    for (const [pattern, replacement] of rules) if (pattern.test(message)) return message.replace(pattern, replacement);
    return message;
  }

  let cliPath = $state(localStorage.getItem('cliPath') ?? '');
  let engineInfo = $state<EngineInfo | null>(null);
  let engineProblem = $state('');
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
  let activeTab = $state<'extract' | 'settings' | 'events'>('extract');
  let unlistenLog: UnlistenFn | undefined;
  let unlistenState: UnlistenFn | undefined;
  let unlistenDrop: UnlistenFn | undefined;

  onMount(() => {
    document.documentElement.lang = language;
    applyAppearance();
    recordEvent('info', 'Aplicación iniciada.', 'Application started.');
    if (!isTauri()) return;
    void (async () => {
      await refreshEngine();
      unlistenLog = await listen<string>('dump-log', (event) => {
        logs = [...logs.slice(-99), event.payload];
        if (/\b(error|warning|warn|falló|fall[oó]|error)\b/i.test(event.payload)) {
          recordEvent(/\b(error|falló|fall[oó])\b/i.test(event.payload) ? 'error' : 'warning', event.payload, localizeError(event.payload));
        }
      });
      unlistenState = await listen<string>('dump-state', (event) => {
        if (event.payload === 'running') phase = 'running';
        if (event.payload === 'done') { phase = 'done'; recordEvent('success', 'Extracción completada.', 'Extraction completed.'); }
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
      filters: [{ name: tr('Contenedores Nintendo Switch', 'Nintendo Switch containers'), extensions: ['nsp', 'xci'] }]
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
      filters: [{ name: tr('Archivos de claves', 'Key files'), extensions: ['keys'] }]
    });
    if (selected) {
      keys = Array.isArray(selected) ? selected : [selected];
      localStorage.setItem('keys', JSON.stringify(keys));
      keyStatus = null;
      invalidatePlan();
    }
  }

  async function chooseOutput() {
    const selected = await open({ directory: true, multiple: false, title: tr('Elegir carpeta de salida', 'Choose output folder') });
    if (typeof selected === 'string') {
      outputDir = selected;
      localStorage.setItem('outputDir', outputDir);
      invalidatePlan();
    }
  }

  async function chooseCli() {
    const selected = await open({ multiple: false, title: tr('Seleccionar ejecutable nsromexefs', 'Select nsromexefs executable') });
    if (typeof selected === 'string') {
      cliPath = selected;
      localStorage.setItem('cliPath', cliPath);
      invalidatePlan();
      await refreshEngine();
    }
  }

  async function refreshEngine() {
    if (!isTauri()) return;
    try {
      const info = await invoke<EngineInfo>('engine_info', { cliPath: cliPath || null });
      engineInfo = info;
      engineProblem = '';
      if (!info.overridden && cliPath !== info.path) {
        cliPath = info.path;
        localStorage.setItem('cliPath', cliPath);
      }
    } catch (error) {
      engineInfo = null;
      engineProblem = localizeError(error);
    }
  }

  function applyCliPath() {
    localStorage.setItem('cliPath', cliPath);
    invalidatePlan();
    void refreshEngine();
  }

  function resetCliOverride() {
    cliPath = '';
    localStorage.removeItem('cliPath');
    invalidatePlan();
    void refreshEngine();
  }

  async function validateKeys() {
    await action(async () => {
      keyStatus = await invoke<Json>('validate_keys', { cliPath, keys });
      recordEvent('success', 'Validación de claves completada.', 'Key validation completed.');
    }, tr('No se pudieron validar las claves. Revisa las rutas y el formato.', 'Could not validate the keys. Check the paths and file format.'));
  }

  async function reviewPlan() {
    await action(async () => {
      recordEvent('info', 'Revisión del plan iniciada.', 'Extraction plan review started.');
      phase = 'reviewing';
      plan = await invoke<Json>('plan_dump', { cliPath, inputs, keys, options: options() });
      phase = 'planned';
      result = null;
      recordEvent('success', 'Plan de extracción preparado.', 'Extraction plan prepared.');
    }, tr('No se pudo preparar el plan. Comprueba los archivos, claves y destino.', 'Could not prepare the plan. Check the input files, keys, and destination.'));
  }

  async function startExtraction() {
    logs = [];
    result = null;
    recordEvent('info', 'Extracción iniciada.', 'Extraction started.');
    await action(async () => {
      result = await invoke<Json>('run_dump', { cliPath, inputs, keys, options: options() });
      phase = 'done';
    }, tr('La extracción no terminó correctamente. Puedes revisar el log y reanudarla.', 'Extraction did not complete. Review the log and resume it.'));
  }

  async function cancelExtraction() {
    try {
      await invoke('cancel_dump');
      phase = 'error';
      errorMessage = tr('Extracción cancelada. El estado queda disponible para reanudar.', 'Extraction cancelled. Its state is available to resume.');
      recordEvent('warning', 'Extracción cancelada por el usuario.', 'Extraction cancelled by the user.');
    } catch (error) {
      const rawMessage = error instanceof Error ? error.message : String(error);
      errorMessage = localizeError(rawMessage);
      recordEvent('error', rawMessage, localizeError(rawMessage));
    }
  }

  async function action(work: () => Promise<void>, fallback: string) {
    busy = true;
    errorMessage = '';
    try {
      if (!engineInfo) throw new Error(tr('No se encontró el motor nsromexefs. Selecciona el ejecutable en Ajustes.', 'No nsromexefs engine was found. Select the executable in Settings.'));
      await work();
    } catch (error) {
      const rawMessage = error instanceof Error ? error.message : String(error || fallback);
      errorMessage = localizeError(rawMessage);
      phase = 'error';
      recordEvent('error', rawMessage, localizeError(rawMessage));
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
    return `${size.toLocaleString(language === 'en' ? 'en-US' : 'es-ES', { maximumFractionDigits: 2 })} ${units[unit]}`;
  }

  function report(value: Json | null) { return value?.titles?.[0] ?? null; }
  function totalBytes(value: Json | null) {
    const item = report(value);
    return (item?.exefs?.bytes ?? 0) + (item?.romfs?.bytes ?? 0);
  }
</script>

<svelte:head>
  <title>nsromexefs · {tr('extracción', 'extraction')}</title>
  <meta name="description" content={tr('Inspecciona y extrae exefs y romfs desde NSP y XCI.', 'Inspect and extract exefs and romfs from NSP and XCI containers.')} />
</svelte:head>

<div class="shell">
  <aside class="rail">
    <div class="brand-mark"><img src="/logo.png" alt={tr('Logotipo de nsromexefs', 'nsromexefs logo')} /></div>
    <button class:active={activeTab === 'extract'} class="rail-button" title={tr('Extracción', 'Extraction')} onclick={() => activeTab = 'extract'}>
      <span class="nav-icon">⇩</span><small>{tr('Extraer', 'Extract')}</small>
    </button>
    <button class:active={activeTab === 'settings'} class="rail-button" title={tr('Ajustes', 'Settings')} onclick={() => activeTab = 'settings'}>
      <span class="nav-icon">⚙</span><small>{tr('Ajustes', 'Settings')}</small>
    </button>
    <button class:active={activeTab === 'events'} class="rail-button" title={tr('Eventos', 'Events')} onclick={() => activeTab = 'events'}>
      <span class="nav-icon">≡</span><small>{tr('Eventos', 'Events')}</small>
    </button>
    <div class="rail-bottom"><span class:ok={!!engineInfo} class="online-dot"></span><small>{engineInfo?.version ? `CLI v${engineInfo.version}` : 'CLI —'}</small></div>
  </aside>

  <main class="main-view">
    <header class="topbar">
      <div class="crumb"><span>NSROMEXEFS</span><b>/</b>{activeTab === 'extract' ? tr('EXTRACCIÓN', 'EXTRACTION') : activeTab === 'settings' ? tr('PREFERENCIAS', 'PREFERENCES') : tr('EVENTOS', 'EVENTS')}</div>
      <div class="top-status"><button class="language-button" aria-label={tr('Cambiar idioma', 'Change language')} onclick={() => setLanguage(language === 'en' ? 'es' : 'en')}>{language === 'en' ? 'EN / ES' : 'ES / EN'}</button><span class:ok={!!engineInfo} class="status-pip"></span>{engineInfo ? tr('Motor conectado', 'Engine connected') : tr('Motor no detectado', 'Engine not detected')}</div>
    </header>

    {#if activeTab === 'extract'}
      <section class="page-heading">
        <div>
          <div class="eyebrow">{tr('HERRAMIENTA DE EXTRACCIÓN', 'EXTRACTION TOOL')}</div>
          <h1>{tr('Prepara tu contenido', 'Prepare your content')}<span class="heading-period">.</span></h1>
          <p>{tr('Selecciona tus archivos, revisa el plan y extrae con seguridad.', 'Select your files, review the plan, and extract safely.')}</p>
        </div>
        <div class="stepper" aria-label={tr('Progreso del flujo', 'Workflow progress')}>
          <div class:current={phase === 'ready' || phase === 'reviewing'} class:complete={!!plan}><i>01</i><span>{tr('Preparar', 'Prepare')}</span></div>
          <div class="step-line"></div>
          <div class:current={phase === 'planned'} class:complete={phase === 'running' || phase === 'done'}><i>02</i><span>{tr('Revisar', 'Review')}</span></div>
          <div class="step-line"></div>
          <div class:current={phase === 'running' || phase === 'done'}><i>03</i><span>{tr('Extraer', 'Extract')}</span></div>
        </div>
      </section>

      <div class="content-grid">
        <section class="workflow-column">
          <article class="card source-card">
            <div class="card-heading">
              <div class="section-number">01</div>
              <div><h2>{tr('Archivos de origen', 'Source files')}</h2><p>{tr('Elige un NSP/XCI, o base y update.', 'Choose an NSP/XCI, or a base game and update.')}</p></div>
            </div>
            <button class="drop-zone" onclick={chooseInputs} disabled={busy}>
              <span class="upload-icon">↑</span>
              <span class="drop-title">{tr('Arrastra o selecciona tus archivos', 'Drop or select your files')}</span>
              <span class="drop-subtitle">{tr('Compatible con .NSP y .XCI · máximo 2 archivos', 'Supports .NSP and .XCI · up to 2 files')}</span>
              <span class="select-button">{tr('Seleccionar archivos', 'Choose files')} <b>↗</b></span>
            </button>
            {#if inputs.length}
              <div class="file-list">
                {#each inputs as input, index}
                  <div class="file-row"><div class="file-badge">{input.toLowerCase().endsWith('.xci') ? 'XCI' : 'NSP'}</div><div class="file-name"><strong>{input.split(/[\\/]/).pop()}</strong><span>{input}</span></div><button class="remove-file" aria-label={tr('Quitar archivo', 'Remove file')} onclick={() => { inputs = inputs.filter((_, i) => i !== index); plan = null; }}>×</button></div>
                {/each}
              </div>
            {/if}
            <div class="card-divider"></div>
            <div class="inline-setting">
              <div class="setting-icon key-icon">⌑</div>
              <div class="setting-copy"><strong>{tr('Archivos de claves', 'Key files')}</strong><span>{keys.length ? tr(`${keys.length} archivo(s) seleccionado(s)`, `${keys.length} file(s) selected`) : tr('prod.keys y title.keys (opcional)', 'prod.keys and title.keys (optional)')}</span></div>
              <button class="text-button" onclick={chooseKeys} disabled={busy}>{keys.length ? tr('Cambiar', 'Change') : tr('Añadir claves', 'Add keys')} <b>＋</b></button>
            </div>
            {#if keys.length}
              <div class="key-list">{#each keys as key}<span class="key-chip">▣ {key.split(/[\\/]/).pop()}</span>{/each}<button class="mini-check" onclick={validateKeys} disabled={busy}>{tr('Validar', 'Validate')}</button></div>
            {/if}
            {#if keyStatus}
              <div class="key-result"><span class="status-pip ok"></span>{tr('Keyset válido', 'Valid keyset')} · {tr('revisión', 'revision')} {keyStatus.max_revision ?? '—'} · {keyStatus.titlekeys ?? 0} titlekeys</div>
            {/if}
          </article>

          <article class="card destination-card">
            <div class="card-heading">
              <div class="section-number">02</div>
              <div><h2>{tr('Destino y opciones', 'Destination and options')}</h2><p>{tr('Define dónde guardar los archivos extraídos.', 'Choose where to save the extracted files.')}</p></div>
            </div>
            <div class="destination-row">
              <label for="destination">{tr('CARPETA DE SALIDA', 'OUTPUT FOLDER')}</label>
              <div class="path-control"><input id="destination" bind:value={outputDir} placeholder={tr('Selecciona una carpeta…', 'Choose a folder…')} oninput={() => { localStorage.setItem('outputDir', outputDir); invalidatePlan(); }} /><button onclick={chooseOutput} aria-label={tr('Buscar carpeta', 'Browse folders')}>↗</button></div>
            </div>
            <div class="options-grid">
              <label class="check-option"><input type="checkbox" bind:checked={tree} onchange={invalidatePlan} /><span class="checkmark"></span><span><strong>{tr('Organizar por título', 'Organize by title')}</strong><small>{tr('Carpetas por versión y DLC', 'Folders by version and DLC')}</small></span></label>
              <label class="check-option"><input type="checkbox" bind:checked={resume} onchange={invalidatePlan} /><span class="checkmark"></span><span><strong>{tr('Reanudar extracción', 'Resume extraction')}</strong><small>{tr('Verifica archivos ya escritos', 'Verify files already written')}</small></span></label>
              <label class="check-option"><input type="checkbox" bind:checked={onlyExefs} onchange={() => { if (onlyExefs) onlyRomfs = false; invalidatePlan(); }} /><span class="checkmark"></span><span><strong>{tr('Solo exefs', 'exefs only')}</strong><small>{tr('Ejecutables y metadatos', 'Executables and metadata')}</small></span></label>
              <label class="check-option"><input type="checkbox" bind:checked={onlyRomfs} onchange={() => { if (onlyRomfs) onlyExefs = false; invalidatePlan(); }} /><span class="checkmark"></span><span><strong>{tr('Solo romfs', 'romfs only')}</strong><small>{tr('Datos del juego', 'Game data')}</small></span></label>
              <label class="check-option"><input type="checkbox" bind:checked={force} onchange={invalidatePlan} /><span class="checkmark"></span><span><strong>{tr('Reemplazar existentes', 'Replace existing files')}</strong><small>{tr('Sobrescribe archivos ya extraídos', 'Overwrite previously extracted files')}</small></span></label>
            </div>
            <div class="destination-row version-row"><label for="version">{tr('VERSIÓN (OPCIONAL)', 'VERSION (OPTIONAL)')}</label><input id="version" bind:value={version} placeholder={tr('Automática · ej. 1.1.0', 'Automatic · e.g. 1.1.0')} oninput={invalidatePlan} /></div>
          </article>

          {#if errorMessage}<div class="alert-error"><span>!</span><div><strong>{tr('No se pudo completar la acción', 'The action could not be completed')}</strong><p>{errorMessage}</p></div><button onclick={() => errorMessage = ''} aria-label={tr('Cerrar', 'Dismiss')}>×</button></div>{/if}

          <div class="action-row">
            {#if phase === 'running'}
              <button class="secondary-button cancel-button" onclick={cancelExtraction}>{tr('Cancelar extracción', 'Cancel extraction')}</button>
            {:else}
              <button class="secondary-button" onclick={reviewPlan} disabled={busy || !inputs.length || !outputDir}>{busy && phase === 'reviewing' ? tr('Analizando…', 'Analyzing…') : tr('Revisar plan', 'Review plan')} <span>→</span></button>
              <button class="primary-button" onclick={startExtraction} disabled={busy || !plan || (phase !== 'planned' && phase !== 'error')}>
                {busy ? tr('Preparando…', 'Preparing…') : phase === 'done' ? tr('Extraer de nuevo', 'Extract again') : tr('Iniciar extracción', 'Start extraction')} <span>↗</span>
              </button>
            {/if}
          </div>
        </section>

        <aside class="summary-column">
          <article class="summary-card">
            <div class="summary-top"><span class="summary-label">{tr('RESUMEN DEL PLAN', 'PLAN SUMMARY')}</span><span class="plan-status" class:ready={!!plan}>{plan ? tr('LISTO', 'READY') : tr('PENDIENTE', 'PENDING')}</span></div>
            {#if report(plan)}
              {@const item = report(plan)}
              <h3>{item.display_version ? `${tr('Versión', 'Version')} ${item.display_version}` : tr('Extracción preparada', 'Extraction ready')}</h3>
              <p class="title-id">{tr('Título', 'Title')} {item.title_id}</p>
              <div class="summary-stats"><div><strong>{item.exefs?.files ?? '—'}</strong><span>exefs {tr('archivos', 'files')}</span></div><div><strong>{item.romfs?.files ?? '—'}</strong><span>romfs {tr('archivos', 'files')}</span></div></div>
              <div class="summary-total"><span>{tr('Datos estimados', 'Estimated data')}</span><strong>{humanBytes(totalBytes(plan))}</strong></div>
              {#if item.merged}<div class="merge-note"><span>⤳</span> BKTR update · {tr('se fusionará con la base', 'will be merged with the base')}</div>{/if}
            {:else}
              <div class="empty-summary"><div class="empty-graphic"><span>⌁</span><i></i></div><strong>{tr('Tu plan aparecerá aquí', 'Your plan will appear here')}</strong><p>{tr('Selecciona los archivos y pulsa “Revisar plan” para ver títulos, versiones y espacio requerido.', 'Select files and choose “Review plan” to see titles, versions, and required disk space.')}</p></div>
            {/if}
            {#if phase === 'planned'}<div class="plan-confirm"><span>✓</span> {tr('Destino y espacio revisados', 'Destination and disk space checked')}</div>{/if}
          </article>

          {#if phase === 'running' || phase === 'done' || phase === 'error'}
            <article class="progress-card">
              <div class="progress-title"><div><span class="summary-label">{tr('ESTADO', 'STATUS')}</span><h3>{phase === 'running' ? tr('Extrayendo…', 'Extracting…') : phase === 'done' ? tr('Extracción lista', 'Extraction complete') : tr('Revisa el resultado', 'Check the result')}</h3></div><span class:spinning={phase === 'running'} class="progress-symbol">{phase === 'done' ? '✓' : phase === 'running' ? '↻' : '!'}</span></div>
              {#if phase === 'running'}<div class="progress-track"><span></span></div><p class="progress-hint">{tr('El proceso puede tardar varios minutos según el tamaño del RomFS.', 'This may take several minutes depending on the size of the RomFS.')}</p>{/if}
              {#if result}<div class="result-details"><div><span>{tr('Archivos con checksum', 'Checksummed files')}</span><b>{report(result)?.checksums ?? '—'}</b></div><div><span>{tr('Tiempo total', 'Total time')}</span><b>{report(result)?.timings?.total_ms ? `${(report(result).timings.total_ms / 1000).toFixed(1)} s` : '—'}</b></div><div><span>Manifest</span><b>{report(result)?.manifest ? tr('Generado', 'Generated') : '—'}</b></div></div>{/if}
              {#if logs.length}<div class="log-box">{#each logs as line}<div>{localizeLogLine(line)}</div>{/each}</div>{/if}
            </article>
          {/if}

          <div class="privacy-note"><span>◈</span><p><strong>{tr('Privado por diseño', 'Private by design')}</strong><br />{tr('Todo el procesamiento ocurre localmente. Tus archivos y claves no se envían a ningún servidor.', 'All processing happens locally. Your files and keys are never sent to a server.')}</p></div>
        </aside>
      </div>
    {:else if activeTab === 'settings'}
      <section class="page-heading settings-heading"><div><div class="eyebrow">{tr('PREFERENCIAS LOCALES', 'LOCAL PREFERENCES')}</div><h1>{tr('Ajustes', 'Settings')}<span class="heading-period">.</span></h1><p>{tr('La aplicación incluye el motor. Ajusta aquí las claves, la apariencia y, si lo necesitas, una CLI alternativa.', 'The app bundles its engine. Manage keys, appearance, and, if needed, an alternative CLI.')}</p></div></section>
      <section class="settings-layout">
        <article class="card settings-card"><div class="card-heading"><div class="section-number">01</div><div><h2>{tr('Motor nsromexefs', 'nsromexefs engine')}</h2><p>{tr('La GUI usa la misma CLI para mantener idéntica lógica de extracción.', 'The GUI uses the same CLI extraction logic.')}</p></div></div>
          <div class="destination-row"><label for="cli">{tr('EJECUTABLE CLI · OPCIONAL', 'CLI EXECUTABLE · OPTIONAL')}</label><div class="path-control"><input id="cli" bind:value={cliPath} placeholder={tr('Detección automática', 'Automatic detection')} onchange={applyCliPath} /><button onclick={chooseCli} aria-label={tr('Buscar ejecutable', 'Browse for executable')}>↗</button></div></div>
          {#if engineInfo?.overridden}<button class="text-button reset-engine" onclick={resetCliOverride}>{tr('Volver al motor incluido', 'Use included engine')}</button>{/if}
          <div class="settings-hint engine-hint" class:missing={!engineInfo}>
            <span class:ok={!!engineInfo} class="status-pip"></span>
            {#if engineInfo}
              <span><strong>{engineInfo.bundled ? tr('Motor incluido con la aplicación', 'Engine bundled with the app') : tr('Motor local detectado', 'Local engine detected')}</strong> · v{engineInfo.version ?? '—'} · {engineInfo.overridden ? tr('ruta personalizada', 'custom path') : tr('detección automática', 'automatic detection')}</span>
            {:else}
              <span>{engineProblem || tr('No se encontró el motor nsromexefs. Selecciona el ejecutable o reinstala la aplicación.', 'No nsromexefs engine was found. Select the executable or reinstall the application.')}</span>
            {/if}
          </div>
        </article>
        <article class="card settings-card"><div class="card-heading"><div class="section-number">02</div><div><h2>{tr('Archivos de claves', 'Key files')}</h2><p>{tr('Las claves se usan localmente y nunca se imprimen ni se suben.', 'Keys are used locally and are never displayed or uploaded.')}</p></div></div>
          <div class="settings-key-row"><div class="setting-copy"><strong>{keys.length ? tr(`${keys.length} archivos configurados`, `${keys.length} file(s) configured`) : tr('No hay archivos configurados', 'No files configured')}</strong><span>{keys.map((key) => key.split(/[\\/]/).pop()).join(' · ') || tr('Puedes cambiar la selección cuando quieras.', 'You can change your selection at any time.')}</span></div><button class="text-button" onclick={chooseKeys}>{tr('Seleccionar', 'Choose')} <b>↗</b></button></div>
          <button class="secondary-button validate-button" onclick={validateKeys} disabled={!keys.length || busy}>{tr('Validar keyset', 'Validate keyset')} <span>→</span></button>
          {#if keyStatus}<div class="key-result settings-key-result"><span class="status-pip ok"></span>{tr('Keyset válido', 'Valid keyset')} · {keyStatus.keys} {tr('claves', 'keys')} · {tr('revisión', 'revision')} {keyStatus.max_revision ?? '—'}</div>{/if}
        </article>
        <article class="card settings-card appearance-card">
          <div class="card-heading"><div class="section-number">03</div><div><h2>{tr('Apariencia', 'Appearance')}</h2><p>{tr('Personaliza los colores y la tipografía. Tus preferencias se guardan en este dispositivo.', 'Choose a color palette and font. Preferences are saved on this device.')}</p></div></div>
          <div class="appearance-controls">
            <label class="appearance-field"><span>{tr('COMBINACIÓN DE COLORES', 'COLOR PALETTE')}</span><select value={theme} onchange={(event) => setTheme(event.currentTarget.value as ThemeName)}>
              <option value="classic">{tr('Rojo clásico y crema', 'Classic red and cream')}</option><option value="ocean">{tr('Océano · azul y turquesa', 'Ocean · blue and teal')}</option><option value="forest">{tr('Bosque · verde y marfil', 'Forest · green and ivory')}</option><option value="midnight">{tr('Medianoche · violeta oscuro', 'Midnight · dark violet')}</option>
            </select></label>
            <label class="appearance-field"><span>{tr('FUENTE', 'FONT')}</span><select value={font} onchange={(event) => setFont(event.currentTarget.value as FontName)}>
              <option value="rounded">{tr('Redondeada', 'Rounded')}</option><option value="system">{tr('Sistema', 'System')}</option><option value="serif">{tr('Serif', 'Serif')}</option><option value="mono">{tr('Monoespaciada', 'Monospace')}</option>
            </select></label>
          </div>
        </article>
        <article class="about-card credits-card"><div class="brand-mark small-mark"><img src="/logo.png" alt={tr('Logotipo de nsromexefs', 'nsromexefs logo')} /></div><div><strong>nsromexefs GUI · {tr('en desarrollo', 'in development')}</strong><p>{tr('Interfaz Tauri 2 · procesamiento local con la CLI MIT.', 'Tauri 2 interface · local processing with the MIT-licensed CLI.')}</p><p>{tr('Este proyecto fue creado con asistencia de herramientas de IA; el código y sus resultados son revisados por el mantenedor.', 'This project was created with assistance from AI tools; the maintainer reviews the code and its output.')}</p><strong>{tr('Referencias técnicas y créditos', 'Technical references and credits')}</strong><ul class="credit-links"><li><a href="https://github.com/SciresM/hactool" target="_blank" rel="noreferrer">hactool · SciresM · ISC</a></li><li><a href="https://github.com/jakcron/nstool" target="_blank" rel="noreferrer">nstool · jakcron · MIT</a></li><li><a href="https://github.com/jakcron/libnintendo-hac" target="_blank" rel="noreferrer">libnintendo-hac · jakcron · MIT</a></li><li><a href="https://switchbrew.org" target="_blank" rel="noreferrer">{tr('Documentación de los formatos de Switchbrew', 'Switchbrew format documentation')}</a></li></ul><p>{tr('Las dependencias y sus licencias están documentadas en', 'Dependencies and their licenses are documented in')} <a href="https://github.com/SirEdmundiumRockwell/nsromexefs/blob/main/THIRD-PARTY-NOTICES.md" target="_blank" rel="noreferrer">THIRD-PARTY-NOTICES.md</a>.</p><a href="https://github.com/SirEdmundiumRockwell/nsromexefs" target="_blank" rel="noreferrer">{tr('Código fuente del proyecto', 'Project source code')} <span>↗</span></a></div></article>
      </section>
    {:else}
      <section class="page-heading settings-heading"><div><div class="eyebrow">{tr('DIAGNÓSTICO LOCAL', 'LOCAL DIAGNOSTICS')}</div><h1>{tr('Registro de eventos', 'Event log')}<span class="heading-period">.</span></h1><p>{tr('Los eventos y errores recientes se guardan solo en este dispositivo.', 'Recent events and errors are stored on this device only.')}</p></div></section>
      <section class="event-log-card card">
        <div class="event-log-toolbar"><div><strong>{tr(`${eventLog.length} de ${EVENT_LOG_LIMIT} eventos`, `${eventLog.length} of ${EVENT_LOG_LIMIT} events`)}</strong><p>{tr('Las rutas personales y los nombres de archivos de claves se ocultan en el registro.', 'Personal paths and key-file names are redacted from the log.')}</p></div><div class="event-log-actions"><button class="secondary-button" onclick={downloadEventLog} disabled={!eventLog.length}>{tr('Descargar registro', 'Download log')} <span>↓</span></button><button class="text-button" onclick={clearEventLog} disabled={!eventLog.length}>{tr('Borrar', 'Clear')}</button></div></div>
        {#if eventLog.length}
          <ol class="event-list">
            {#each [...eventLog].reverse() as event}
              <li class="event-entry"><span class="event-level" class:error={event.level === 'error'} class:warning={event.level === 'warning'} class:success={event.level === 'success'}>{tr(event.level === 'error' ? 'ERROR' : event.level === 'warning' ? 'AVISO' : event.level === 'success' ? 'OK' : 'INFO', event.level.toUpperCase())}</span><time datetime={event.at}>{new Date(event.at).toLocaleString(language === 'en' ? 'en-US' : 'es-ES')}</time><p>{language === 'en' ? event.en : event.es}</p></li>
            {/each}
          </ol>
        {:else}
          <div class="event-empty"><strong>{tr('Todavía no hay eventos', 'No events yet')}</strong><p>{tr('Las operaciones y errores aparecerán aquí cuando uses la aplicación.', 'Operations and errors will appear here as you use the app.')}</p></div>
        {/if}
      </section>
    {/if}
    <footer><span>NSROMEXEFS <i>·</i> {tr('HERRAMIENTA DE EXTRACCIÓN', 'EXTRACTION TOOL')}</span><span>{tr('Hecho para procesar localmente', 'Built for local processing')} <b>◈</b></span></footer>
  </main>
</div>
