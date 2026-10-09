<!-- The chat on the right, as Code Wiki's: a question about the code, answered by Claude reading it, the answer
     streaming in as markdown with links into the code, a line for each file it reads, and follow-ups in the
     same conversation. The section being read goes with each question. An exported page can't ask, and says so.
     Carried over from crystal's wiki page (assets/wiki/app.js). -->
<script lang="ts">
  import { tick } from 'svelte';
  import { ask } from '$lib/api';
  import Icon from '$lib/components/Icon.svelte';
  import type { Mode } from '$lib/codelinks';
  import { money } from '$lib/format';
  import { escapeHtml, renderInlineMarkdown, type MarkdownOptions } from '$lib/markdown';
  import type { Drawing } from '$lib/mermaid';
  import Prose from './Prose.svelte';

  let {
    mode,
    repoKey,
    repoName,
    options,
    section,
    suggestions = [],
    onzoom,
    onclose,
    oncodeclick,
  }: {
    mode: Mode;
    repoKey: string | null;
    repoName: string;
    options: MarkdownOptions;
    /** The id of the section in view, sent with a question. */
    section: () => string | null;
    suggestions?: string[];
    onzoom: (drawing: Drawing, caption: string) => void;
    onclose: () => void;
    oncodeclick: (event: MouseEvent) => void;
  } = $props();

  interface Tool {
    verb: string;
    html: string;
  }
  interface Message {
    role: 'user' | 'bot';
    text: string;
    tools: Tool[];
    error: string;
    done: boolean;
    stopped: boolean;
    cost: number | null;
  }

  const TOOL_WORDS: Record<string, string> = { Read: 'Reading', Grep: 'Searching for', Glob: 'Looking for', LS: 'Listing', Bash: 'Running' };

  let messages = $state<Message[]>([]);
  let conversation: string | null = null;
  let question = $state('');
  let busy = $state(false);
  let abort: AbortController | null = null;
  let log: HTMLElement;
  let input: HTMLTextAreaElement;
  const canAsk = $derived(mode === 'serve' && !!repoKey);
  const ready = $derived(busy || question.trim().length > 0);

  const nearBottom = () => log.scrollHeight - log.scrollTop - log.clientHeight < 80;
  const toBottom = () => tick().then(() => (log.scrollTop = log.scrollHeight));

  export function focus() {
    input?.focus();
  }

  function toolLine(data: { name: string; path?: string; pattern?: string; command?: string }): Tool {
    const verb = TOOL_WORDS[data.name] || data.name || 'Using a tool';
    const html = data.path
      ? renderInlineMarkdown(`[\`${data.path.replace(/`/g, '')}\`](code:${data.path.replace(/[()\s]/g, encodeURIComponent)})`, options)
      : `<code>${escapeHtml(data.pattern || data.command || '')}</code>`;
    return { verb, html };
  }

  async function send(text: string) {
    if (!canAsk || busy || !repoKey) return;
    messages.push({ role: 'user', text, tools: [], error: '', done: true, stopped: false, cost: null });
    messages.push({ role: 'bot', text: '', tools: [], error: '', done: false, stopped: false, cost: null });
    const bot = messages[messages.length - 1];
    busy = true;
    abort = new AbortController();
    void toBottom();
    let ended = false;
    try {
      await ask(
        repoKey,
        { question: text, conversation, section: section() },
        (event) => {
          const stick = nearBottom();
          if (event.event === 'delta') bot.text += event.data.text || '';
          else if (event.event === 'tool') bot.tools.push(toolLine(event.data));
          else if (event.event === 'done') {
            ended = true;
            conversation = event.data.conversation || conversation;
            bot.cost = typeof event.data.cost_usd === 'number' ? event.data.cost_usd : null;
          } else if (event.event === 'error') {
            ended = true;
            bot.error = event.data.message || 'Something went wrong answering that.';
          }
          if (stick) void toBottom();
        },
        abort.signal,
      );
      if (!ended) bot.error = bot.text ? 'The answer stopped before it was done.' : 'lattice closed the stream without an answer.';
    } catch (err) {
      if ((err as Error).name === 'AbortError') bot.stopped = true;
      else bot.error = `Couldn't ask: ${(err as Error).message}`;
    } finally {
      // Its last lines (a diagram drawn, what it cost) come as it ends: still in sight if the answer was.
      const stick = nearBottom();
      bot.done = true;
      busy = false;
      abort = null;
      if (stick) void toBottom();
    }
  }

  function submit(event?: SubmitEvent) {
    event?.preventDefault();
    if (busy) {
      abort?.abort();
      return;
    }
    const text = question.trim();
    if (!text) return;
    question = '';
    void tick().then(grow);
    void send(text);
  }

  function grow() {
    if (!input) return;
    input.style.height = 'auto';
    input.style.height = `${Math.min(input.scrollHeight, 168)}px`;
  }

  function startOver() {
    abort?.abort();
    conversation = null;
    messages = [];
    input?.focus();
  }
</script>

<aside class="chat" aria-label="Chat about {repoName}">
  <div class="chat-head">
    {#if messages.length}
      <button class="round small" type="button" title="New conversation" aria-label="New conversation" onclick={startOver}><Icon name="new" size={20} /></button>
    {/if}
    <button class="round small" type="button" title="Hide the chat" aria-label="Hide the chat" onclick={onclose}><Icon name="panel" size={20} /></button>
  </div>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
  <div class="chat-log" bind:this={log} role="log" aria-live="polite" onclick={oncodeclick}>
    {#if !messages.length}
      <div class="chat-empty">
        <svg class="chat-spark" viewBox="0 0 24 24" aria-hidden="true">
          <defs>
            <linearGradient id="lw-spark" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#f2b48f" /><stop offset="1" stop-color="#c15f3c" /></linearGradient>
          </defs>
          <path fill="url(#lw-spark)" d="M12 1.5c.5 5.6 4.9 10 10.5 10.5-5.6.5-10 4.9-10.5 10.5C11.5 16.9 7.1 12.5 1.5 12 7.1 11.5 11.5 7.1 12 1.5z" />
        </svg>
        <div class="chat-hello">Hi there!</div>
        <div class="chat-sub">Ask me any questions about the codebase</div>
        {#if !canAsk}
          <div class="chat-static">Asking needs <code>lattice serve</code>, which reads the code to answer.</div>
        {:else if suggestions.length}
          <div class="suggest">
            {#each suggestions as s (s)}<button type="button" onclick={() => send(s)}>{s}</button>{/each}
          </div>
        {/if}
      </div>
    {/if}
    {#each messages as msg, i (i)}
      {#if msg.role === 'user'}
        <div class="msg user">{msg.text}</div>
      {:else}
        <div class="msg bot">
          {#if msg.tools.length}
            <ul class="tools" aria-label="What Claude read">
              {#each msg.tools as tool, j (j)}
                <li><Icon name="file" size={15} /><span>{tool.verb} {@html tool.html}</span></li>
              {/each}
            </ul>
          {/if}
          {#if msg.text}
            <Prose md={msg.text} {options} diagrams={msg.done} drawNow {onzoom} />
          {/if}
          {#if !msg.done && !msg.text}
            <div class="typing" role="status" aria-label="Thinking"><i></i><i></i><i></i></div>
          {/if}
          {#if msg.error}<div class="msg-error" role="alert">{msg.error}</div>{/if}
          {#if msg.stopped}<div class="meta">Stopped.</div>{/if}
          {#if msg.done && msg.cost !== null}<div class="meta">{money(msg.cost)}</div>{/if}
        </div>
      {/if}
    {/each}
  </div>
  <form class="chat-form" onsubmit={submit}>
    <textarea
      bind:this={input}
      bind:value={question}
      rows="1"
      placeholder={canAsk ? 'Ask about this repository' : 'Asking needs lattice serve'}
      aria-label="Ask about this repository"
      disabled={!canAsk}
      oninput={grow}
      onkeydown={(e) => {
        if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
          e.preventDefault();
          submit();
        }
      }}
    ></textarea>
    <button class="send" class:ready={ready && canAsk} type="submit" disabled={!canAsk || !ready} aria-label={busy ? 'Stop' : 'Ask'} title={busy ? 'Stop' : 'Ask'}>
      <Icon name={busy ? 'stop' : 'send'} />
    </button>
  </form>
  <p class="chat-note">Claude reads this repository to answer, and can make mistakes, so double-check it.</p>
</aside>
