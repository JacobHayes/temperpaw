<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import {
    fetchSecretsSchema,
    fetchSetupStatus,
    getSecret,
    saveSecret,
    deleteSecret,
    listSecretKeys,
    connectDiscord,
    disconnectDiscord,
    connectSlack,
    disconnectSlack,
    fetchOpenAICodexStatus,
    startOpenAICodexDeviceLogin,
    pollOpenAICodexDeviceLogin,
    disconnectOpenAICodexAuth,
    type SecretSchemaEntry,
    type SetupStatus,
    type OpenAICodexAuthStatus,
  } from '$lib/api';
  import { changePassword } from '$lib/auth';

  interface VarRow {
    key: string;
    label: string;
    category: string;
    description: string;
    value: string;
    filled: boolean;
    editing: boolean;
    draft: string;
    saving: boolean;
    isCustom: boolean;
    sensitive: boolean;
  }

  type Feedback = { type: 'error' | 'success'; message: string };

  let loading = $state(true);
  let status = $state<SetupStatus | null>(null);
  let vars = $state<VarRow[]>([]);
  // Feedback is keyed by "slot" so each message renders inline next to the
  // control that produced it (a provider card, a variable row, the add form,
  // or the page itself) instead of one global banner at the top of the page.
  let feedback = $state<Record<string, Feedback | null>>({});
  const feedbackTimers: Record<string, ReturnType<typeof setTimeout>> = {};

  // Add variable form
  let addingVar = $state(false);
  let newKey = $state('');
  let newValue = $state('');

  // Service connect states
  let connectingDiscord = $state(false);
  let connectingSlack = $state(false);
  let connectingCodex = $state(false);
  let codexStatus = $state<OpenAICodexAuthStatus | null>(null);
  // Codex device-code sign-in flow
  let codexStarting = $state(false); // device-code request in flight (before code appears)
  let codexPolling = $state(false); // a poll request (manual or background) is in flight
  let codexPollTimer: ReturnType<typeof setInterval> | null = null;
  let codexPollDeadlineMs = 0;
  const CODEX_POLL_INTERVAL_MS = 5000; // backend advertises poll_interval_ms=5000
  const CODEX_POLL_MAX_WALL_MS = 15 * 60 * 1000; // hard stop after 15 min

  // Entity states where a device code is pending user authorization.
  const CODEX_PENDING_STATES = ['Starting', 'DeviceCodeReady', 'Polling'];
  function codexIsAwaitingCode(s: OpenAICodexAuthStatus | null): boolean {
    return !!s?.user_code && !s.configured && CODEX_PENDING_STATES.includes(s.status ?? '');
  }
  // Set once the device code's expiry passes. The backend entity stays in
  // DeviceCodeReady holding the stale code, so without this flag the UI would
  // keep showing the dead code and leave "Sign in" disabled forever.
  let codexExpired = $state(false);
  // Only show the device-code block while genuinely awaiting authorization. Once
  // the flow completes (Ready/configured) the entity keeps the stale user_code,
  // so gating on this derived value clears the block on success — and on expiry,
  // re-enabling "Sign in" (StartDeviceLogin mints a fresh code).
  let codexAwaitingCode = $derived(!codexExpired && codexIsAwaitingCode(codexStatus));

  // Account
  let apiKey = $state('');
  let showApiKey = $state(false);
  let currentPassword = $state('');
  let newPassword = $state('');

  // Keys that should be masked (actual secrets). Non-secret config values show plain.
  const SENSITIVE_KEYS = new Set([
    'anthropic_api_key', 'openai_api_key', 'openai_codex_access_token', 'openai_codex_refresh_token', 'openai_codex_token', 'openrouter_api_key',
    'huggingface_api_key', 'hf_token', 'fireworks_api_key', 'sakana_fugu_api_key', 'openai_compatible_api_key', 'openai_compatible_headers_json',
    'discord_bot_token', 'slack_app_token', 'slack_bot_token', 'slack_signing_secret',
    'github_token', 'exa_api_key', 'tensorlake_api_key', 'modal_token_id', 'modal_token_secret', 'dd_api_key', 'dd_app_key', 'temper_api_key',
  ]);

  function showFeedback(slot: string, type: 'error' | 'success', message: string) {
    feedback[slot] = { type, message };
    feedback = { ...feedback };
    if (feedbackTimers[slot]) clearTimeout(feedbackTimers[slot]);
    if (type === 'success') {
      feedbackTimers[slot] = setTimeout(() => {
        feedback[slot] = null;
        feedback = { ...feedback };
      }, 4000);
    }
  }

  function clearFeedback(slot: string) {
    if (feedbackTimers[slot]) clearTimeout(feedbackTimers[slot]);
    feedback[slot] = null;
    feedback = { ...feedback };
  }

  async function copyDiscordInteractionUrl() {
    const interactionUrl = status?.discord_interaction_url;
    if (!interactionUrl) return;
    try {
      await navigator.clipboard.writeText(interactionUrl);
      showFeedback('discord', 'success', 'Copied Discord Interaction URL. Paste it into Discord Developer Portal -> General Information -> Interactions Endpoint URL.');
    } catch {
      showFeedback('discord', 'error', 'Failed to copy the Discord Interaction URL');
    }
  }

  onMount(async () => {
    await load();
  });

  async function load() {
    loading = true;
    try {
      const [schema, existingKeys, nextStatus, savedApiKey] = await Promise.all([
        fetchSecretsSchema(),
        listSecretKeys(),
        fetchSetupStatus(),
        getSecret('temper_api_key'),
      ]);
      status = nextStatus;
      codexStatus = await fetchOpenAICodexStatus().catch(() => null);
      apiKey = savedApiKey ?? '';

      const schemaKeys = new Set(schema.map(s => s.key));
      const rows: VarRow[] = [];

      // Fetch values for all schema keys in parallel
      const valueResults = await Promise.all(
        schema.map(s => getSecret(s.key).then(v => ({ key: s.key, value: v })))
      );
      const valueMap = new Map(valueResults.map(r => [r.key, r.value]));

      for (const s of schema) {
        const val = valueMap.get(s.key) ?? '';
        rows.push({
          key: s.key,
          label: s.label,
          category: s.category,
          description: s.description,
          value: val,
          filled: !!val,
          editing: false,
          draft: '',
          saving: false,
          isCustom: false,
          sensitive: SENSITIVE_KEYS.has(s.key),
        });
      }

      // Add any custom keys not in schema
      for (const k of existingKeys) {
        if (!schemaKeys.has(k)) {
          const val = await getSecret(k);
          rows.push({
            key: k,
            label: k,
            category: 'custom',
            description: '',
            value: val ?? '',
            filled: !!val,
            editing: false,
            draft: '',
            saving: false,
            isCustom: true,
            sensitive: true,
          });
        }
      }

      vars = rows;
    } catch (err) {
      showFeedback('page', 'error', err instanceof Error ? err.message : 'Failed to load');
    } finally {
      loading = false;
    }
  }

  function startEdit(row: VarRow) {
    row.editing = true;
    row.draft = row.value;
    vars = [...vars];
  }

  function cancelEdit(row: VarRow) {
    row.editing = false;
    row.draft = '';
    vars = [...vars];
  }

  async function saveVar(row: VarRow) {
    if (!row.draft.trim()) return;
    row.saving = true;
    vars = [...vars];
    try {
      await saveSecret(row.key, row.draft.trim());
      row.value = row.draft.trim();
      row.filled = true;
      row.editing = false;
      row.draft = '';
      status = await fetchSetupStatus();
      if (row.key.startsWith('discord_') && status?.discord_connected) {
        showFeedback(row.key, 'success', `${row.key} saved and Discord reconnected`);
      } else {
        showFeedback(row.key, 'success', `${row.key} saved`);
      }
    } catch (err) {
      showFeedback(row.key, 'error', `Failed to save ${row.key}: ${err instanceof Error ? err.message : 'unknown'}`);
    } finally {
      row.saving = false;
      vars = [...vars];
    }
  }

  async function removeVar(row: VarRow) {
    row.saving = true;
    vars = [...vars];
    try {
      await deleteSecret(row.key);
      if (row.isCustom) {
        vars = vars.filter(v => v.key !== row.key);
      } else {
        row.value = '';
        row.filled = false;
        row.editing = false;
        row.saving = false;
        vars = [...vars];
      }
      status = await fetchSetupStatus();
    } catch (err) {
      row.saving = false;
      vars = [...vars];
      showFeedback(row.key, 'error', `Failed to delete ${row.key}`);
    }
  }

  async function addVar() {
    const k = newKey.trim();
    const v = newValue.trim();
    if (!k || !v) return;
    try {
      await saveSecret(k, v);
      vars = [...vars, {
        key: k, label: k, category: 'custom', description: '',
        value: v, filled: true, editing: false, draft: '', saving: false,
        isCustom: true, sensitive: true,
      }];
      newKey = '';
      newValue = '';
      addingVar = false;
      showFeedback('add', 'success', `${k} added`);
    } catch (err) {
      showFeedback('add', 'error', `Failed to add ${k}: ${err instanceof Error ? err.message : 'unknown'}`);
    }
  }

  // ── Provider registry ──
  // Maps a secret key to the provider card it belongs to. Rules are ordered
  // most-specific-first (e.g. `openai_codex*` and `openai_compatible*` before the
  // bare `openai_api_key`). `category` lets us re-home keys that arrive as
  // "custom" (e.g. dd_* observability keys that aren't in the backend schema)
  // into the right section. Returns null for keys with no known provider — those
  // collect into an "Other" card inside their own category so nothing is lost.
  interface ProviderInfo { id: string; name: string; category: string; }
  function resolveProvider(key: string): ProviderInfo | null {
    const llm = (id: string, name: string): ProviderInfo => ({ id, name, category: 'llm' });
    if (key.startsWith('openai_codex')) return llm('codex', 'OpenAI Codex');
    if (key.startsWith('openai_compatible')) return llm('openai_compatible', 'OpenAI-Compatible');
    if (key === 'openai_api_key') return llm('openai', 'OpenAI');
    if (key.startsWith('anthropic_')) return llm('anthropic', 'Anthropic');
    if (key.startsWith('openrouter_')) return llm('openrouter', 'OpenRouter');
    if (key.startsWith('huggingface_') || key === 'hf_token') return llm('huggingface', 'Hugging Face');
    if (key.startsWith('fireworks_')) return llm('fireworks', 'Fireworks');
    if (key.startsWith('sakana')) return llm('sakana', 'Sakana');
    if (key.startsWith('local_openai')) return llm('local_openai', 'Local (Ollama)');
    if (key === 'llm_provider' || key === 'llm_model') return llm('llm_active', 'Active Model');
    if (key.startsWith('discord_')) return { id: 'discord', name: 'Discord', category: 'messaging' };
    if (key.startsWith('slack_')) return { id: 'slack', name: 'Slack', category: 'messaging' };
    if (key.startsWith('exa_')) return { id: 'exa', name: 'Exa', category: 'web_search' };
    if (key.startsWith('modal_')) return { id: 'modal', name: 'Modal', category: 'sandbox' };
    if (key.startsWith('tensorlake_')) return { id: 'tensorlake', name: 'TensorLake', category: 'sandbox' };
    if (key === 'sandbox_provider') return { id: 'sandbox_active', name: 'Active Provider', category: 'sandbox' };
    if (key.startsWith('github_')) return { id: 'github', name: 'GitHub', category: 'integrations' };
    if (key.startsWith('dd_')) return { id: 'datadog', name: 'Datadog', category: 'observability' };
    return null;
  }

  // Ordering of provider cards within a category. Indices only need to be unique
  // per category. Unknown providers (incl. the "Other" fallback) sort last.
  const PROVIDER_ORDER: Record<string, number> = {
    anthropic: 0, openai: 1, codex: 2, openrouter: 3, huggingface: 4, fireworks: 5,
    sakana: 6, openai_compatible: 7, local_openai: 8, llm_active: 9,
    discord: 0, slack: 1,
    exa: 0,
    modal: 0, tensorlake: 1, sandbox_active: 2,
    github: 0,
    datadog: 0,
  };

  interface ProviderCardData {
    id: string;
    name: string;
    category: string;
    special: '' | 'discord' | 'slack' | 'codex';
    rows: VarRow[];
  }

  // A provider card's status dot: connection-aware for the transports/OAuth
  // providers, otherwise "on" when any of its keys is set.
  function cardOn(card: ProviderCardData): boolean {
    if (card.special === 'discord') return !!status?.discord_connected;
    if (card.special === 'slack') return !!status?.slack_connected;
    if (card.special === 'codex') return !!codexStatus?.configured;
    return card.rows.some(r => r.filled);
  }

  // Group every variable into per-provider cards, then group cards by category.
  let groupedProviders = $derived.by(() => {
    const catOrder = ['llm', 'web_search', 'sandbox', 'messaging', 'integrations', 'observability', 'custom'];
    const cats = new Map<string, Map<string, ProviderCardData>>();
    for (const v of vars) {
      const info = resolveProvider(v.key);
      const category = info?.category ?? v.category ?? 'custom';
      const id = info?.id ?? 'other';
      const name = info?.name ?? 'Other';
      if (!cats.has(category)) cats.set(category, new Map());
      const providers = cats.get(category)!;
      if (!providers.has(id)) {
        const special = id === 'discord' ? 'discord' : id === 'slack' ? 'slack' : id === 'codex' ? 'codex' : '';
        providers.set(id, { id, name, category, special, rows: [] });
      }
      providers.get(id)!.rows.push(v);
    }
    const orderIdx = (id: string) => PROVIDER_ORDER[id] ?? 99;
    const result: Array<{ category: string; cards: ProviderCardData[] }> = [];
    const emit = (category: string) => {
      const providers = cats.get(category);
      if (!providers) return;
      const cards = [...providers.values()].sort(
        (a, b) => orderIdx(a.id) - orderIdx(b.id) || a.name.localeCompare(b.name),
      );
      result.push({ category, cards });
    };
    for (const c of catOrder) emit(c);
    for (const c of cats.keys()) if (!catOrder.includes(c)) emit(c);
    return result;
  });

  // Discord connect
  async function handleConnectDiscord() {
    connectingDiscord = true;
    clearFeedback('discord');
    try {
      const botToken = vars.find(v => v.key === 'discord_bot_token')?.value ?? '';
      const publicKey = vars.find(v => v.key === 'discord_public_key')?.value ?? '';
      const guildId = vars.find(v => v.key === 'discord_guild_id')?.value ?? '';
      const feedChannel = vars.find(v => v.key === 'discord_feed_channel_id')?.value ?? '';
      const forumChannel = vars.find(v => v.key === 'discord_forum_channel_id')?.value ?? '';
      const interactionDelivery = (vars.find(v => v.key === 'discord_interaction_delivery')?.value || status?.discord_interaction_delivery || 'gateway') as 'gateway' | 'webhook';
      if (!botToken) { showFeedback('discord', 'error', 'Set discord_bot_token first'); return; }
      const result = await connectDiscord({
        bot_token: botToken,
        public_key: publicKey || undefined,
        guild_id: guildId || undefined,
        feed_channel_id: feedChannel || undefined,
        forum_channel_id: forumChannel || undefined,
        interaction_delivery: interactionDelivery,
      });
      status = await fetchSetupStatus();
      const interactionUrl = result.discord_interaction_url ?? status?.discord_interaction_url;
      if (interactionUrl) {
        showFeedback('discord', 'success', 'Discord connected in Interaction URL mode. Copy the URL below into Discord Developer Portal -> General Information -> Interactions Endpoint URL, then save.');
      } else {
        showFeedback('discord', 'success', 'Discord connected in Gateway mode. Clear the Interaction URL in Discord Developer Portal so interactions arrive over the Gateway.');
      }
    } catch (err) {
      showFeedback('discord', 'error', err instanceof Error ? err.message : 'Discord connection failed');
    } finally { connectingDiscord = false; }
  }

  async function handleDisconnectDiscord() {
    connectingDiscord = true;
    try {
      await disconnectDiscord();
      status = await fetchSetupStatus();
      showFeedback('discord', 'success', 'Discord disconnected');
    } catch (err) {
      showFeedback('discord', 'error', err instanceof Error ? err.message : 'Failed');
    } finally { connectingDiscord = false; }
  }

  async function handleConnectSlack() {
    connectingSlack = true;
    clearFeedback('slack');
    try {
      const appToken = vars.find(v => v.key === 'slack_app_token')?.value ?? '';
      const botToken = vars.find(v => v.key === 'slack_bot_token')?.value ?? '';
      const signing = vars.find(v => v.key === 'slack_signing_secret')?.value ?? '';
      if (!appToken || !botToken) { showFeedback('slack', 'error', 'Set slack_app_token and slack_bot_token first'); return; }
      await connectSlack({
        app_token: appToken, bot_token: botToken,
        signing_secret: signing || undefined,
      });
      status = await fetchSetupStatus();
      showFeedback('slack', 'success', 'Slack connected');
    } catch (err) {
      showFeedback('slack', 'error', err instanceof Error ? err.message : 'Slack connection failed');
    } finally { connectingSlack = false; }
  }

  async function handleDisconnectSlack() {
    connectingSlack = true;
    try {
      await disconnectSlack();
      status = await fetchSetupStatus();
      showFeedback('slack', 'success', 'Slack disconnected');
    } catch (err) {
      showFeedback('slack', 'error', err instanceof Error ? err.message : 'Failed');
    } finally { connectingSlack = false; }
  }

  async function handleStartCodexLogin() {
    codexStarting = true;
    clearFeedback('codex');
    try {
      codexStatus = await startOpenAICodexDeviceLogin();
      codexExpired = false;
      if (codexStatus.user_code) {
        // No banner here: the device-code block below already explains what to
        // do, and a transient duplicate above it makes the layout jump.
        startCodexBackgroundPoll();
      } else {
        showFeedback('codex', 'success', 'OpenAI Codex device login started');
      }
    } catch (err) {
      showFeedback('codex', 'error', err instanceof Error ? err.message : 'OpenAI Codex login failed');
    } finally { codexStarting = false; }
  }

  function codexExpiryMs(): number {
    const raw = codexStatus?.expires_at_ms;
    const parsed = raw ? Number(raw) : NaN;
    return Number.isFinite(parsed) && parsed > 0 ? parsed : 0;
  }

  function stopCodexBackgroundPoll() {
    if (codexPollTimer !== null) {
      clearInterval(codexPollTimer);
      codexPollTimer = null;
    }
  }

  function startCodexBackgroundPoll() {
    stopCodexBackgroundPoll();
    const cap = Date.now() + CODEX_POLL_MAX_WALL_MS;
    const expiry = codexExpiryMs();
    codexPollDeadlineMs = expiry > 0 ? Math.min(expiry, cap) : cap;
    codexPollTimer = setInterval(() => { void codexBackgroundTick(); }, CODEX_POLL_INTERVAL_MS);
  }

  async function codexBackgroundTick() {
    if (codexPolling) return; // don't overlap in-flight polls
    if (Date.now() >= codexPollDeadlineMs) {
      stopCodexBackgroundPoll();
      codexExpired = true;
      showFeedback('codex', 'error', 'OpenAI Codex device code expired. Start sign-in again.');
      return;
    }
    await runCodexPoll(false);
  }

  // Shared poll used by both the manual "Check" button and the background timer.
  async function runCodexPoll(manual: boolean): Promise<void> {
    if (codexPolling) return;
    codexPolling = true;
    if (manual) clearFeedback('codex');
    try {
      const next = await pollOpenAICodexDeviceLogin();
      codexStatus = next;
      if (next.configured) {
        stopCodexBackgroundPoll();
        status = await fetchSetupStatus();
        showFeedback('codex', 'success', 'OpenAI Codex connected');
        await load();
        return;
      }
      if (next.status === 'Failed') {
        stopCodexBackgroundPoll();
        showFeedback('codex', 'error', next.last_error || 'OpenAI Codex sign-in failed');
      }
    } catch (err) {
      // Surface manual errors; swallow transient background errors and keep
      // retrying until the deadline.
      if (manual) {
        showFeedback('codex', 'error', err instanceof Error ? err.message : 'OpenAI Codex polling failed');
      }
    } finally {
      codexPolling = false;
    }
  }

  async function handleDisconnectCodex() {
    connectingCodex = true;
    stopCodexBackgroundPoll();
    try {
      codexStatus = await disconnectOpenAICodexAuth();
      status = await fetchSetupStatus();
      await load();
      showFeedback('codex', 'success', 'OpenAI Codex disconnected');
    } catch (err) {
      showFeedback('codex', 'error', err instanceof Error ? err.message : 'OpenAI Codex disconnect failed');
    } finally { connectingCodex = false; }
  }

  // Resume background polling if a device-code flow is already pending on mount
  // (e.g. the user navigated away and came back), and always clean up on destroy.
  $effect(() => {
    if (!codexAwaitingCode) return;
    const expiry = codexExpiryMs();
    if (expiry > 0 && Date.now() >= expiry) {
      // Already expired when we got here (e.g. the user stepped away and
      // reloaded the page): skip polling and unlock "Sign in" right away.
      codexExpired = true;
      return;
    }
    if (codexPollTimer === null) startCodexBackgroundPoll();
  });

  onDestroy(() => {
    stopCodexBackgroundPoll();
  });

  async function updatePassword() {
    clearFeedback('account');
    try {
      await changePassword(currentPassword, newPassword);
      currentPassword = '';
      newPassword = '';
      showFeedback('account', 'success', 'Password updated');
    } catch (err) {
      showFeedback('account', 'error', err instanceof Error ? err.message : 'Failed');
    }
  }

  function mask(val: string): string {
    if (!val) return '';
    if (val.length <= 8) return '\u2022'.repeat(val.length);
    return val.slice(0, 4) + '\u2022'.repeat(Math.min(val.length - 4, 12));
  }

  const CAT_LABELS: Record<string, string> = {
    llm: 'LLM',
    web_search: 'Web Search',
    sandbox: 'Sandbox',
    messaging: 'Messaging',
    integrations: 'Integrations',
    observability: 'Observability',
    custom: 'Custom',
  };

  // Anchor id for deep-linking a provider card. Provider ids are unique across
  // categories except the "Other" fallback, which gets category-qualified.
  function cardAnchor(card: ProviderCardData): string {
    return card.id === 'other' ? `${card.category}-other` : card.id;
  }
</script>

<svelte:head>
  <title>Settings &middot; Temper Paw</title>
</svelte:head>

<div class="page">
  <h1 class="page-title">SETTINGS</h1>

  <!-- Inline feedback slot: renders a message adjacent to the control that
       produced it, keyed by `slot`. -->
  {#snippet fbSlot(slot: string)}
    {@const fb = feedback[slot]}
    {#if fb}
      <div class="slot-fb" class:slot-fb--err={fb.type === 'error'}>{fb.message}</div>
    {/if}
  {/snippet}

  <!-- A single editable variable row + its own inline feedback slot. -->
  {#snippet varRow(row: VarRow)}
    <div class="var-row">
      <div class="var-main">
        <span class="var-dot" class:var-dot--on={row.filled}></span>
        <span class="var-key">{row.key}</span>
        <span class="var-val">
          {#if row.filled}
            {row.sensitive ? mask(row.value) : row.value}
          {:else}
            <span class="var-unset">--</span>
          {/if}
        </span>
        <div class="var-actions">
          {#if row.editing}
            <button class="act" onclick={() => cancelEdit(row)} disabled={row.saving}>Cancel</button>
          {:else}
            <button class="act" onclick={() => startEdit(row)}>{row.filled ? 'Edit' : 'Set'}</button>
            {#if row.filled}
              <button class="act act-danger" onclick={() => removeVar(row)} disabled={row.saving}>Del</button>
            {/if}
          {/if}
        </div>
      </div>
      {#if row.description && !row.editing}
        <div class="var-desc">{row.description}</div>
      {/if}
      {#if row.editing}
        <form class="var-edit" onsubmit={(e) => { e.preventDefault(); saveVar(row); }}>
          <input
            class="var-input"
            type={row.sensitive ? 'password' : 'text'}
            bind:value={row.draft}
            placeholder={row.sensitive ? 'Enter value' : row.value || 'Enter value'}
            disabled={row.saving}
          />
          <button class="btn-sm" type="submit" disabled={!row.draft.trim() || row.saving}>
            {row.saving ? '...' : 'Save'}
          </button>
        </form>
      {/if}
      {@render fbSlot(row.key)}
    </div>
  {/snippet}

  <!-- Generic provider card: status dot, name, its variable rows and an inline
       feedback slot. Providers that need extra controls (Discord/Slack Connect,
       Codex device-code sign-in) are rendered bespoke below instead. -->
  {#snippet providerCard(card: ProviderCardData)}
    <div class="provider-card" id={cardAnchor(card)}>
      <div class="provider-head">
        <span class="cat-dot" class:cat-dot--on={cardOn(card)}></span>
        <span class="provider-name"><a class="anchor" href="#{cardAnchor(card)}">{card.name}</a></span>
      </div>
      {@render fbSlot(card.id)}
      {#each card.rows as row (row.key)}
        {@render varRow(row)}
      {/each}
    </div>
  {/snippet}

  {#if loading}
    <p class="dim">Loading...</p>
  {:else}
    {@render fbSlot('page')}

    <!-- Provider cards grouped by category; every category and card is a
         deep-linkable anchor target. -->
    <div class="var-list">
      {#each groupedProviders as group}
        <section class="cat-group" id={group.category}>
        <h2 class="cat-title"><a class="anchor" href="#{group.category}">{CAT_LABELS[group.category] ?? group.category}</a></h2>

        {#each group.cards as card (card.category + ':' + card.id)}
          {#if card.special === 'discord'}
            <!-- Discord: Connect/Disconnect, fields, interaction URL. -->
            <div class="provider-card" id={cardAnchor(card)}>
              <div class="provider-head">
                <span class="cat-dot" class:cat-dot--on={status?.discord_connected}></span>
                <span class="provider-name"><a class="anchor" href="#{cardAnchor(card)}">Discord</a></span>
                <div class="provider-actions">
                  {#if status?.discord_connected}
                    <button class="cat-act" onclick={handleDisconnectDiscord} disabled={connectingDiscord}>Disconnect</button>
                  {:else}
                    <button class="cat-act" onclick={handleConnectDiscord} disabled={connectingDiscord}>Connect</button>
                  {/if}
                </div>
              </div>
              {@render fbSlot('discord')}
              <div class="provider-hint">Saving Discord credentials applies them immediately. Use Connect only to retry manually. Enable <strong>Message Content Intent</strong> in Discord Developer Portal &rarr; Bot — the Gateway requires it.</div>
              {#each card.rows as row (row.key)}
                {@render varRow(row)}
              {/each}
              {#if status?.discord_interaction_delivery}
                <div class="cat-hint">
                  <div class="interaction-copy-row">
                    <span class="interaction-copy-label">Discord Interaction Delivery</span>
                  </div>
                  <code class="interaction-url">{status.discord_interaction_delivery}</code>
                  <div>{status.discord_interaction_delivery === 'gateway' ? 'Clear the Interactions Endpoint URL in Discord Developer Portal. Discord will deliver slash commands and buttons over the Gateway.' : 'Discord will POST interactions to the public Interactions Endpoint URL.'}</div>
                </div>
              {/if}
              {#if status?.discord_interaction_url}
                <div class="cat-hint">
                  <div class="interaction-copy-row">
                    <span class="interaction-copy-label">Discord Interaction URL</span>
                    <button class="act" onclick={copyDiscordInteractionUrl}>Copy</button>
                  </div>
                  <code class="interaction-url">{status.discord_interaction_url}</code>
                  <div>Paste this into Discord Developer Portal -> General Information -> Interactions Endpoint URL, then click Save Changes.</div>
                </div>
              {/if}
            </div>

          {:else if card.special === 'slack'}
            <!-- Slack: Connect/Disconnect and fields. -->
            <div class="provider-card" id={cardAnchor(card)}>
              <div class="provider-head">
                <span class="cat-dot" class:cat-dot--on={status?.slack_connected}></span>
                <span class="provider-name"><a class="anchor" href="#{cardAnchor(card)}">Slack</a></span>
                <div class="provider-actions">
                  {#if status?.slack_connected}
                    <button class="cat-act" onclick={handleDisconnectSlack} disabled={connectingSlack}>Disconnect</button>
                  {:else}
                    <button class="cat-act" onclick={handleConnectSlack} disabled={connectingSlack}>Connect</button>
                  {/if}
                </div>
              </div>
              {@render fbSlot('slack')}
              {#each card.rows as row (row.key)}
                {@render varRow(row)}
              {/each}
            </div>

          {:else if card.special === 'codex'}
            <!-- OpenAI Codex: device-code sign-in plus the managed token rows. -->
            <div class="provider-card" id={cardAnchor(card)}>
              <div class="provider-head">
                <span class="cat-dot" class:cat-dot--on={codexStatus?.configured}></span>
                <span class="provider-name"><a class="anchor" href="#{cardAnchor(card)}">OpenAI Codex</a></span>
                <div class="provider-actions">
                  {#if codexStatus?.configured}
                    <button class="cat-act" onclick={handleDisconnectCodex} disabled={connectingCodex}>Disconnect</button>
                  {:else}
                    <button class="cat-act" onclick={handleStartCodexLogin} disabled={codexStarting || codexAwaitingCode}>
                      {#if codexStarting}
                        <span class="spinner spinner--sm"></span> Requesting code…
                      {:else if codexAwaitingCode}
                        Awaiting code…
                      {:else}
                        Sign in
                      {/if}
                    </button>
                  {/if}
                </div>
              </div>
              {@render fbSlot('codex')}
              {#if codexAwaitingCode}
                <div class="cat-hint">
                  <div class="interaction-copy-row">
                    <span class="interaction-copy-label">OpenAI Codex Device Code</span>
                    <button class="act" onclick={() => runCodexPoll(true)} disabled={codexPolling}>
                      {codexPolling ? 'Checking…' : 'Check'}
                    </button>
                  </div>
                  <code class="interaction-url">{codexStatus?.user_code}</code>
                  <div><a href={codexStatus?.verification_url} target="_blank" rel="noreferrer">{codexStatus?.verification_url}</a></div>
                  <div class="codex-poll-status">
                    <span class="spinner spinner--sm"></span>
                    Open the link, enter the code, and authorize — checking automatically every 5s.
                  </div>
                </div>
              {/if}
              {#each card.rows as row (row.key)}
                {@render varRow(row)}
              {/each}
            </div>

          {:else}
            {@render providerCard(card)}
          {/if}
        {/each}
        </section>
      {/each}

      <!-- Add variable -->
      {#if addingVar}
        <form class="add-form" onsubmit={(e) => { e.preventDefault(); addVar(); }}>
          <input class="var-input var-input-key" bind:value={newKey} placeholder="KEY_NAME" />
          <input class="var-input" type="password" bind:value={newValue} placeholder="Value" />
          <button class="btn-sm" type="submit" disabled={!newKey.trim() || !newValue.trim()}>Add</button>
          <button class="act" type="button" onclick={() => { addingVar = false; newKey = ''; newValue = ''; }}>Cancel</button>
        </form>
      {:else}
        <button class="add-btn" onclick={() => addingVar = true}>+ Add variable</button>
      {/if}
      {@render fbSlot('add')}
    </div>

    <!-- Account -->
    <section class="cat-group section" id="account">
      <h2 class="cat-title"><a class="anchor" href="#account">Account</a></h2>
      <div class="acct-row">
        <span class="var-key">temper_api_key</span>
        <span class="var-val">{showApiKey ? apiKey : mask(apiKey)}</span>
        <button class="act" onclick={() => showApiKey = !showApiKey}>{showApiKey ? 'Hide' : 'Show'}</button>
      </div>
      <form class="pw-form" onsubmit={(e) => { e.preventDefault(); updatePassword(); }}>
        <input class="var-input" type="password" bind:value={currentPassword} placeholder="Current password" />
        <input class="var-input" type="password" bind:value={newPassword} placeholder="New password" />
        <button class="btn-sm" type="submit" disabled={!currentPassword || !newPassword}>Change password</button>
      </form>
      {@render fbSlot('account')}
    </section>
  {/if}
</div>

<style>
  .page { max-width: 720px; }

  .page-title {
    font-family: var(--font-sans);
    font-size: var(--text-xl);
    color: var(--text-1);
    margin-bottom: var(--sp-6);
    letter-spacing: 0.1em;
  }

  .dim {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--text-3);
  }

  /* ── Inline feedback slot ── */
  .slot-fb {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    line-height: 1.4;
    color: var(--accent);
    padding: var(--sp-1) var(--sp-2);
    border-left: 2px solid var(--accent);
    background: rgba(52,211,153,0.06);
    border-radius: var(--radius);
    margin: var(--sp-1) 0;
    word-break: break-word;
  }

  .slot-fb--err {
    color: var(--status-error);
    border-left-color: var(--status-error);
    background: rgba(248,113,113,0.06);
  }

  /* ── Var list ── */
  .var-list {
    display: flex;
    flex-direction: column;
    margin-bottom: var(--sp-6);
  }

  /* ── Category group box ── */
  .cat-group {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--sp-3);
    margin-bottom: var(--sp-4);
    scroll-margin-top: var(--sp-6);
  }

  .cat-title {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--text-2);
    letter-spacing: 0.1em;
    text-transform: uppercase;
    margin: 0 0 var(--sp-2) 0;
  }

  /* Category and provider titles double as deep links; a trailing # appears
     on hover to signal the anchor. */
  .anchor {
    color: inherit;
    text-decoration: none;
  }

  .anchor:hover::after {
    content: ' #';
    color: var(--text-3);
  }

  .cat-group:target,
  .provider-card:target {
    border-color: var(--text-2);
  }

  /* ── Provider subsection card ── */
  .provider-card {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--sp-2) var(--sp-3);
    margin: 0 0 var(--sp-2) 0;
    scroll-margin-top: var(--sp-6);
  }

  .provider-card:last-child {
    margin-bottom: 0;
  }

  .provider-head {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    min-height: 24px;
  }

  .provider-name {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--text-1);
    letter-spacing: 0.04em;
  }

  .provider-actions {
    display: flex;
    gap: var(--sp-2);
    margin-left: auto;
  }

  .provider-hint {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--text-3);
    opacity: 0.8;
    padding: var(--sp-1) 0;
  }

  .cat-act {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--text-2);
    background: none;
    border: 1px solid var(--border);
    cursor: pointer;
    padding: 2px var(--sp-2);
    border-radius: var(--radius);
  }

  .cat-act:hover { color: var(--text-1); border-color: var(--text-2); }
  .cat-act:disabled { opacity: 0.3; cursor: not-allowed; }

  .cat-dot {
    width: 5px; height: 5px;
    border-radius: 50%;
    background: var(--text-3);
    opacity: 0.4;
  }

  .cat-dot--on {
    background: var(--accent);
    opacity: 1;
  }

  .cat-hint {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--text-3);
    padding: 0 0 var(--sp-1) 0;
    word-break: break-all;
  }

  .codex-poll-status {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin-top: var(--sp-1);
    color: var(--text-3);
  }

  .spinner {
    display: inline-block;
    width: 10px;
    height: 10px;
    border: 1.5px solid var(--text-3);
    border-top-color: var(--text-1);
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
    flex-shrink: 0;
    vertical-align: middle;
  }

  .spinner--sm {
    width: 8px;
    height: 8px;
    border-width: 1px;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  /* ── Variable row ── */
  .var-row {
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--border);
    padding: var(--sp-1) 0;
  }

  .var-main {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    min-height: 26px;
  }

  .var-dot {
    width: 6px; height: 6px;
    border-radius: 50%;
    background: var(--text-3);
    flex-shrink: 0;
    opacity: 0.3;
  }

  .var-dot--on {
    background: var(--accent);
    opacity: 1;
  }

  .var-key {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--text-1);
    white-space: nowrap;
    min-width: 0;
  }

  .var-val {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--text-2);
    margin-left: auto;
    text-align: right;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 200px;
  }

  .interaction-copy-row {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    justify-content: space-between;
    margin-bottom: var(--sp-1);
  }

  .interaction-copy-label {
    color: var(--text-2);
  }

  .interaction-url {
    display: block;
    color: var(--text-1);
    margin-bottom: var(--sp-1);
    word-break: break-all;
  }

  .var-unset {
    color: var(--text-3);
    opacity: 0.4;
  }

  .var-desc {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--text-3);
    padding-left: calc(6px + var(--sp-2));
    opacity: 0.7;
  }

  .var-actions {
    display: flex;
    gap: var(--sp-1);
    flex-shrink: 0;
    margin-left: var(--sp-2);
  }

  .act {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--text-3);
    background: none;
    border: none;
    cursor: pointer;
    padding: 2px var(--sp-1);
    border-radius: var(--radius);
  }

  .act:hover { color: var(--text-1); }
  .act:disabled { opacity: 0.3; cursor: not-allowed; }
  .act-danger:hover { color: var(--status-error); }

  /* ── Inline edit ── */
  .var-edit {
    display: flex;
    gap: var(--sp-2);
    padding: var(--sp-1) 0 0 calc(6px + var(--sp-2));
  }

  .var-input {
    flex: 1;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--sp-1) var(--sp-2);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--text-1);
  }

  .var-input::placeholder { color: var(--text-3); }
  .var-input:focus { outline: none; border-color: var(--text-2); }

  .var-input-key { max-width: 200px; text-transform: uppercase; }

  .btn-sm {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    letter-spacing: 0.06em;
    padding: var(--sp-1) var(--sp-3);
    border-radius: var(--radius);
    border: 1px solid var(--text-1);
    background: var(--text-1);
    color: var(--bg);
    font-weight: 600;
    cursor: pointer;
    white-space: nowrap;
  }

  .btn-sm:disabled { opacity: 0.25; cursor: not-allowed; }

  /* ── Add variable ── */
  .add-form {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-2) 0;
    border-top: 1px solid var(--border);
  }

  .add-btn {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--text-3);
    background: none;
    border: none;
    cursor: pointer;
    padding: var(--sp-2) 0;
    text-align: left;
  }

  .add-btn:hover { color: var(--text-1); }

  /* ── Section ── */
  .section {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    padding-bottom: var(--sp-4);
  }

  .acct-row {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-1) 0;
  }

  .acct-row .var-val { flex: 1; text-align: left; }

  .pw-form {
    display: flex;
    gap: var(--sp-2);
    align-items: center;
    padding: var(--sp-1) 0;
  }

  @media (max-width: 640px) {
    .page { max-width: 100%; }
    .var-main { flex-wrap: wrap; }
    .var-val { max-width: none; text-align: left; margin-left: 0; width: 100%; padding-left: calc(6px + var(--sp-2)); }
    .var-actions { width: 100%; padding-left: calc(6px + var(--sp-2)); }
    .provider-head { flex-wrap: wrap; }
    .pw-form { flex-direction: column; align-items: stretch; }
    .var-edit { padding-left: 0; flex-direction: column; }
    .add-form { flex-wrap: wrap; }
    .var-input-key { max-width: none; }
    .acct-row { flex-wrap: wrap; }
    .acct-row .var-val { flex: none; width: 100%; }
  }
</style>
