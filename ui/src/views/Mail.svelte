<script lang="ts">
  import EmptyState from '../EmptyState.svelte';
  import Kindling from '../Kindling.svelte';
  import { art } from '../illustrations';
  import Select from '../Select.svelte';
  import * as api from '../api';
  import { formatReceived, formatSize } from '../format';
  import { createMailStore, links } from '../mail.svelte';

  type Tab = 'html' | 'text' | 'raw' | 'links';

  const store = createMailStore(api);
  let tab = $state<Tab>('html');
  let raw = $state<{ text: string; truncated: boolean } | null>(null);
  let port = $state<number | null>(null);
  let copied = $state(false);
  let showEnv = $state(false);

  // `?mail=<id>` opens a mail directly (used for screenshots in the browser demo).
  const requested = new URLSearchParams(location.search).get('mail');
  store.load().then(() => {
    port = store.status?.port ?? null;
    if (requested) open(requested);
  });

  // New mail shows up while the page is open.
  $effect(() => {
    const timer = setInterval(() => store.refresh(), 2000);
    return () => clearInterval(timer);
  });

  const found = $derived(store.current ? links(store.current.html, store.current.text) : []);

  async function open(id: string) {
    raw = null;
    await store.open(id);
    tab = store.current?.html ? 'html' : 'text';
  }

  async function show(next: Tab) {
    tab = next;
    if (next === 'raw' && !raw) raw = await store.raw();
  }

  function pickInbox(value: string) {
    store.filter = value === '*' ? undefined : value === '' ? null : value;
  }

  async function copyEnv() {
    await store.copyEnv();
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

  async function changePort(event: SubmitEvent) {
    event.preventDefault();
    if (port) await store.setPort(port);
  }

  const openLink = (url: string) => {
    if (/^https?:\/\//i.test(url)) api.openUrl(url);
  };

  /** Links in the mail open in the browser; nothing else in it reacts to a click. */
  function wire(frame: HTMLIFrameElement) {
    const doc = frame.contentDocument;
    if (!doc) return;
    doc.addEventListener(
      'click',
      (e) => {
        const a = (e.target as Element | null)?.closest?.('a');
        if (!a) return;
        e.preventDefault();
        openLink(a.getAttribute('href')?.trim() ?? '');
      },
      true,
    );
  }

  const inboxValue = $derived(store.filter === undefined ? '*' : (store.filter ?? ''));
</script>

<h1>Mail</h1>

<section class="card catcher">
  <div class="status">
    <span class="dot" class:ok={store.status?.listening} class:bad={!!store.status?.error} aria-hidden="true"></span>
    {#if store.status?.error}
      <span class="error">{store.status.error}</span>
    {:else if store.status?.listening}
      <span>Catching mail on <code>127.0.0.1:{store.status.port}</code>. Nothing is sent on.</span>
    {:else}
      <span class="muted">The mail catcher is not running.</span>
    {/if}
  </div>
  <div class="tools">
    <form class="port" onsubmit={changePort}>
      <label for="mail-port">Port</label>
      <input id="mail-port" type="number" min="1" max="65535" bind:value={port} required />
      <button type="submit" disabled={!port || (port === store.status?.port && store.status?.listening)}>Use</button>
    </form>
    <button onclick={() => (showEnv = !showEnv)} aria-expanded={showEnv}>.env lines</button>
    <button onclick={copyEnv}>{copied ? 'Copied' : 'Copy .env'}</button>
  </div>
  {#if store.portError}<p class="error wide" role="alert">{store.portError}</p>{/if}
  {#if showEnv && store.status}
    <div class="wide">
      <pre class="env">{store.status.env}</pre>
      <p class="hint">The username sorts mail by app. Without one, mail lands under "Without a name".</p>
    </div>
  {/if}
</section>

{#if store.error}<p class="error banner" role="alert">{store.error}</p>{/if}

<section class="mailbox" class:reading={!!store.current}>
  <div class="card list" aria-label="Mails">
    <div class="list-head">
      <span class="pick">
        <Select
          label="Inbox"
          value={inboxValue}
          onchange={pickInbox}
          options={[
            { value: '*', label: `All inboxes (${store.unread} unread)` },
            ...store.inboxes.map((inbox) => ({ value: inbox.site ?? '', label: `${inbox.label} (${inbox.count})` })),
          ]}
        />
      </span>
      <button onclick={() => store.clear()} disabled={store.mails.length === 0}>Clear</button>
    </div>
    {#if !store.loaded}
      <Kindling rows={3} label="Loading mail" />
    {:else if store.mails.length === 0}
      <EmptyState art={art.mail} title="The inbox is empty" line="Mail your sites send lands here" />
    {/if}
    <ul>
      {#each store.mails as m (m.id)}
        <li>
          <button class="mail" class:selected={store.current?.id === m.id} class:unread={!m.seen} onclick={() => open(m.id)}>
            <span class="line">
              <span class="from">{m.from}</span>
              <span class="when">{formatReceived(m.received)}</span>
            </span>
            <span class="line">
              <span class="subject">{m.subject}</span>
              {#if m.attachments > 0}<span class="clip" title="{m.attachments} attachment(s)">{m.attachments} file{m.attachments === 1 ? '' : 's'}</span>{/if}
              {#if store.filter === undefined && m.site}<span class="tag">{m.site}</span>{/if}
            </span>
          </button>
        </li>
      {/each}
    </ul>
  </div>

  <div class="card reader">
    {#if store.current}
      {@const mail = store.current}
      <header>
        <div class="title">
          <button class="back" onclick={() => store.close()}>Back</button>
          <h2>{mail.subject}</h2>
          <button onclick={() => store.remove(mail.id)}>Delete</button>
        </div>
        <dl>
          <dt>From</dt><dd>{mail.from}</dd>
          <dt>To</dt><dd>{mail.to.join(', ')}</dd>
          {#if mail.cc.length > 0}<dt>Cc</dt><dd>{mail.cc.join(', ')}</dd>{/if}
          <dt>Received</dt><dd>{new Date(mail.received * 1000).toLocaleString('en-GB')}{mail.site ? ` from ${mail.site}` : ''}</dd>
        </dl>
        {#if mail.files.length > 0}
          <div class="files">
            {#each mail.files as file (file.index)}
              <span class="file">
                <span>{file.name} <span class="muted">{formatSize(file.size)}</span></span>
                <button onclick={() => store.saveAttachment(file)}>Save</button>
              </span>
            {/each}
          </div>
        {/if}
        <div class="tabs" role="tablist">
          {#if mail.html}<button role="tab" aria-selected={tab === 'html'} onclick={() => show('html')}>HTML</button>{/if}
          <button role="tab" aria-selected={tab === 'text'} onclick={() => show('text')}>Text</button>
          <button role="tab" aria-selected={tab === 'raw'} onclick={() => show('raw')}>Raw</button>
          <button role="tab" aria-selected={tab === 'links'} onclick={() => show('links')}>Links ({found.length})</button>
        </div>
      </header>
      <div class="body">
        {#if tab === 'html' && mail.html}
          <!-- No allow-scripts: nothing in a mail runs. Same origin only so links can be caught. -->
          <iframe title="Mail" sandbox="allow-same-origin" srcdoc={mail.html} onload={(e) => wire(e.currentTarget as HTMLIFrameElement)}></iframe>
        {:else if tab === 'text' || (tab === 'html' && !mail.html)}
          <pre>{mail.text ?? 'This mail has no text.'}</pre>
        {:else if tab === 'raw'}
          <pre>{raw?.text ?? 'Loading...'}</pre>
          {#if raw?.truncated}<p class="hint pad">Only the first megabyte is shown.</p>{/if}
        {:else}
          {#if found.length === 0}<p class="empty">No links in this mail.</p>{/if}
          <ul class="links">
            {#each found as link (link.url)}
              <li>
                <span class="link-text">
                  {#if link.text}<strong>{link.text}</strong>{/if}
                  <span class="url">{link.url}</span>
                </span>
                <button onclick={() => openLink(link.url)}>Open</button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    {:else}
      <p class="empty">Pick a mail to read it.</p>
    {/if}
  </div>
</section>

<style>
  h1 {
    margin: 0 0 16px;
    font-size: 22px;
    font-weight: 650;
  }
  h2 {
    flex: 1;
    min-width: 0;
    margin: 0;
    font-size: 16px;
    font-weight: 650;
    overflow-wrap: anywhere;
  }
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 12px;
  }
  .catcher {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 10px 16px;
    margin-bottom: 16px;
    padding: 12px 16px;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    font-size: 13px;
  }
  .tools,
  .port {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .port label {
    color: var(--muted);
    font-size: 13px;
  }
  .port input {
    width: 84px;
  }
  .wide {
    flex-basis: 100%;
    margin: 0;
  }
  .env {
    margin: 0;
    padding: 10px 12px;
    border-radius: 8px;
    background: var(--surface-2);
    font-family: ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace;
    font-size: 12px;
    white-space: pre-wrap;
  }
  .hint {
    margin: 6px 0 0;
    color: var(--muted);
    font-size: 12px;
  }
  .hint.pad {
    padding: 0 16px 12px;
  }
  .mailbox {
    display: grid;
    grid-template-columns: minmax(260px, 340px) minmax(0, 1fr);
    gap: 16px;
    height: max(440px, calc(100vh - 200px));
  }
  .list,
  .reader {
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
  }
  .list-head {
    display: flex;
    gap: 6px;
    padding: 10px;
    border-bottom: 1px solid var(--border);
  }
  .list-head .pick {
    flex: 1;
    min-width: 0;
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .list ul {
    overflow: auto;
  }
  .list li + li {
    border-top: 1px solid var(--border);
  }
  button.mail {
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 100%;
    padding: 9px 12px;
    border: 0;
    border-radius: 0;
    background: transparent;
    text-align: left;
    white-space: normal;
  }
  button.mail:hover {
    background: var(--surface-2);
  }
  button.mail.selected {
    background: var(--nav-active);
  }
  .line {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
  }
  .from,
  .subject {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .from {
    font-size: 13px;
  }
  .subject {
    color: var(--muted);
    font-size: 13px;
  }
  .unread .from,
  .unread .subject {
    color: var(--text);
    font-weight: 650;
  }
  .when,
  .clip {
    flex: none;
    color: var(--muted);
    font-size: 12px;
  }
  .tag {
    flex: none;
    padding: 0 7px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
  }
  header {
    padding: 14px 16px 0;
    border-bottom: 1px solid var(--border);
  }
  .title {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .back {
    display: none;
  }
  dl {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 2px 12px;
    margin: 10px 0;
    font-size: 13px;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .files {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-bottom: 10px;
  }
  .file {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 3px 4px 3px 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
    font-size: 13px;
  }
  .file button {
    padding: 2px 10px;
  }
  .tabs {
    display: flex;
    gap: 2px;
  }
  .tabs button {
    border: 0;
    border-bottom: 2px solid transparent;
    border-radius: 0;
    background: transparent;
    color: var(--muted);
  }
  .tabs button[aria-selected='true'] {
    border-bottom-color: var(--accent);
    color: var(--text);
    font-weight: 600;
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  iframe {
    display: block;
    width: 100%;
    height: 100%;
    border: 0;
    /* Mails are written for a white page. */
    background: #ffffff;
  }
  pre {
    margin: 0;
    padding: 14px 16px;
    font-family: ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace;
    font-size: 12px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .links li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 16px;
  }
  .links li + li {
    border-top: 1px solid var(--border);
  }
  .link-text {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
    font-size: 13px;
  }
  .url {
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  .empty {
    margin: 0;
    padding: 18px 16px;
    color: var(--muted);
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--muted);
  }
  .dot.ok {
    background: var(--ok);
  }
  .dot.bad {
    background: var(--danger);
  }
  .error {
    color: var(--danger);
  }
  .banner {
    margin: 0 0 12px;
  }
  .muted {
    color: var(--muted);
  }
  code {
    font-family: ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace;
    font-size: 12px;
  }
  input {
    min-width: 0;
    padding: 5px 8px;
    border: 1px solid var(--border);
    border-radius: 7px;
    background: var(--surface);
    color: var(--text);
    font: inherit;
  }
  button:disabled {
    cursor: default;
    opacity: 0.55;
  }
  /* Narrow windows: the list, or the open mail with a way back. */
  @media (max-width: 1000px) {
    .mailbox {
      grid-template-columns: minmax(0, 1fr);
    }
    .mailbox.reading .list,
    .mailbox:not(.reading) .reader {
      display: none;
    }
    .back {
      display: inline-block;
    }
  }
</style>
