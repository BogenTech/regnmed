<script>
  // Meldinger (docs/meldinger.md): samtaler én-til-én, som en vanlig
  // meldingsapp. Til venstre samtalene, nyeste melding øverst; til høyre
  // den åpne samtalen. På telefon er det én av dem om gangen, og adressen
  // (#/meldinger/{id}) avgjør hvilken — tilbake-knappen virker som ventet.
  //
  // Hvem man kan skrive til avgjør serveren (kolleger, byråets
  // kontaktperson, klientene, selskapets egne agenter). En melding til en
  // agent er en oppgave; svaret kommer tilbake i samme samtale. Vedlegg
  // legges i innboksen til et selskap begge jobber i før meldingen sendes.
  import { tick, untrack } from "svelte";
  import { api, post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { chat, refreshUnread } from "../../lib/chat.svelte.js";
  import { formatTime } from "../../lib/format.js";
  import { download } from "../../lib/download.js";
  import Agentansikt from "../../components/Agentansikt.svelte";
  import UserMenu from "../../components/UserMenu.svelte";

  // Inside a company the messenger keeps the company's menu around it
  // (#/c/{id}/meldinger); from the byrå, the platform or the company list
  // it stands on its own (#/meldinger). Same conversations either way --
  // they belong to the person, not to the company.
  let { conversationId = null, companyId = null } = $props();
  const base = $derived(companyId ? "#/c/" + companyId + "/meldinger" : "#/meldinger");

  // When list and conversation stand side by side. In the shell the
  // company menu already takes 12rem, so two panes wait for a wider
  // screen; until then it is one at a time, as on a phone. Literal class
  // names, because Tailwind only builds the ones it can read here.
  const pane = $derived(
    companyId
      ? { wide: "lg:flex", narrow: "lg:hidden", list: "lg:w-80" }
      : { wide: "sm:flex", narrow: "sm:hidden", list: "sm:w-80" },
  );

  // Where the frame starts on the page, for its height inside the shell.
  let frame = $state(null);
  let top = $state(0);
  $effect(() => {
    if (!companyId || !frame) return;
    const measure = () => (top = frame.getBoundingClientRect().top + window.scrollY);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(document.body);
    return () => observer.disconnect();
  });

  const NEW = "ny";
  const isNew = $derived(conversationId === NEW);
  const openId = $derived(conversationId && conversationId !== NEW ? conversationId : null);

  let conversations = $state(null);
  let listError = $state(null);
  let filter = $state("");
  let contacts = $state(null);
  let contactFilter = $state("");
  let thread = $state(null); // { me, with, attach_to, messages }
  let threadError = $state(null);
  let text = $state("");
  let files = $state([]);
  let attachTo = $state(null);
  let sending = $state(false);
  let scroller = $state(null);
  let input = $state(null);
  let fileInput = $state(null);
  let search = $state(null);
  let lastMarked = "";

  // ---- Labels ----

  const VIA = {
    colleague: (l) => "Kollega · " + l.company,
    agent: (l) => "Agent · " + l.company,
    accountant: (l) => "Regnskapsfører · " + l.firm,
    firm_colleague: (l) => "Kollega · " + l.firm,
    client: (l) => "Kunde · " + l.company,
  };
  function via(c) {
    const first = c.links[0];
    const label = VIA[first.via]?.(first) ?? "";
    return c.links.length > 1 ? label + " +" + (c.links.length - 1) : label;
  }
  // Today: the time. Earlier: the date. The way every messenger does it.
  function when(iso) {
    const d = new Date(iso);
    const now = new Date();
    const p = (n) => String(n).padStart(2, "0");
    return d.toDateString() === now.toDateString()
      ? p(d.getHours()) + ":" + p(d.getMinutes())
      : p(d.getDate()) + "." + p(d.getMonth() + 1) + ".";
  }
  const STATUS = {
    ny: "har fått oppgaven",
    pagar: "jobber med det …",
    sporsmal: "har et spørsmål",
    utfort: "er ferdig",
    avvist: "avviste oppgaven",
  };

  // ---- The list ----

  async function loadList() {
    try {
      conversations = (await api("/chat/conversations")).conversations;
      listError = null;
    } catch (e) {
      listError = e.message;
    }
  }

  const shown = $derived(
    (conversations || []).filter((c) =>
      c.with.name.toLowerCase().includes(filter.trim().toLowerCase()),
    ),
  );

  // ---- New conversation ----

  async function loadContacts() {
    try {
      contacts = (await api("/chat/contacts")).contacts;
    } catch (e) {
      toast(e.message, false);
      contacts = [];
    }
  }

  const contactsShown = $derived(
    (contacts || []).filter((c) =>
      (c.name + " " + via(c)).toLowerCase().includes(contactFilter.trim().toLowerCase()),
    ),
  );

  async function start(c) {
    try {
      const opened = await post("/chat/conversations", {
        person_id: c.person_id,
        company_id: c.agent ? c.company_id : null,
      });
      location.hash = base + "/" + opened.conversation_id;
    } catch (e) {
      toast(e.message, false);
    }
  }

  // The full explanation lives in the docs; the banner only says what the
  // reader needs right now.
  const AGENT_DOK =
    "https://github.com/BogenTech/regnmed/blob/main/docs/agent.md#hvor-kj%C3%B8rer-agenten";

  // ---- The open conversation ----

  function nearBottom() {
    return !scroller || scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 80;
  }

  async function loadThread(full) {
    const id = openId;
    if (!id) return;
    try {
      const newest =
        !full && thread?.messages.length ? thread.messages[thread.messages.length - 1].created_at : null;
      const svar = await api(
        "/chat/conversations/" + id + (newest ? "?since=" + encodeURIComponent(newest) : ""),
      );
      if (id !== openId) return;
      // Follow the newest message only when the reader is already there;
      // the 30 s backstop must not pull someone out of the history.
      const stick = !thread || nearBottom();
      if (full || !thread) {
        thread = svar;
      } else {
        // `since` overlaps by a few seconds on purpose: keep one of each,
        // and let a newer copy (an agent's status moved) win.
        const known = new Map(thread.messages.map((m) => [m.message_id, m]));
        for (const m of svar.messages) known.set(m.message_id, m);
        thread.messages = [...known.values()].sort((a, b) =>
          a.created_at === b.created_at
            ? a.message_id.localeCompare(b.message_id)
            : a.created_at.localeCompare(b.created_at),
        );
      }
      if (!attachTo || !thread.attach_to.includes(attachTo)) attachTo = thread.attach_to[0] || null;
      threadError = null;
      if (stick) {
        await tick();
        scroller?.scrollTo({ top: scroller.scrollHeight });
      }
      markRead();
    } catch (e) {
      if (id === openId) threadError = e.message;
    }
  }

  // A long thread arrives one page at a time: the newest first, older
  // pages as the reader scrolls up. The scroll position is held, so the
  // message being read does not jump away under the reader.
  let lasterEldre = $state(false);
  async function loadOlder() {
    const id = openId;
    const first = thread?.messages[0];
    if (!id || !first || !thread.older || lasterEldre) return;
    lasterEldre = true;
    try {
      const svar = await api(
        "/chat/conversations/" +
          id +
          "?before=" +
          encodeURIComponent(first.created_at) +
          "&before_id=" +
          first.message_id,
      );
      if (id !== openId) return;
      const fra = scroller ? scroller.scrollHeight - scroller.scrollTop : 0;
      const known = new Set(thread.messages.map((m) => m.message_id));
      thread.messages = [...svar.messages.filter((m) => !known.has(m.message_id)), ...thread.messages];
      thread.older = svar.older;
      await tick();
      if (scroller) scroller.scrollTop = scroller.scrollHeight - fra;
    } catch (e) {
      if (id === openId) threadError = e.message;
    } finally {
      lasterEldre = false;
    }
  }

  function onScroll() {
    if (scroller && scroller.scrollTop < 120) loadOlder();
  }

  async function markRead() {
    const last = thread?.messages[thread.messages.length - 1];
    if (!last || document.visibilityState !== "visible" || last.created_at <= lastMarked) return;
    lastMarked = last.created_at;
    try {
      await post("/chat/conversations/" + openId + "/read", { until: last.created_at });
      refreshUnread();
      const row = conversations?.find((c) => c.conversation_id === openId);
      if (row) row.unread = 0;
    } catch {
      lastMarked = "";
    }
  }

  // ---- Wiring ----

  // Another conversation: start it fresh.
  $effect(() => {
    openId;
    untrack(() => {
      thread = null;
      threadError = null;
      text = "";
      files = [];
      attachTo = null;
      lastMarked = "";
      loadThread(true);
    });
  });
  $effect(() => {
    if (isNew) untrack(() => {
      contactFilter = "";
      loadContacts();
    });
  });
  // The keyboard goes where the next thing is typed: the search when
  // choosing who to write to, the message box when a conversation opens.
  // Not on a touch screen, where focus means a keyboard covering half
  // the conversation.
  const finePointer = matchMedia("(pointer: fine)").matches;
  $effect(() => {
    if (isNew && search) search.focus();
  });
  // Once per opened conversation, not on every refresh of it.
  const threadReady = $derived(!!thread);
  $effect(() => {
    if (finePointer && openId && threadReady && input) untrack(() => input.focus());
  });
  // The live stream says "look now" -- and this also runs once on open,
  // which is the list's first load.
  $effect(() => {
    chat.tick;
    untrack(() => {
      loadList();
      loadThread(false);
    });
  });
  // Backstop: an event can be lost, and an agent's status on an older
  // message only changes on a full read.
  $effect(() => {
    const timer = setInterval(() => {
      loadList();
      loadThread(true);
    }, 30_000);
    return () => clearInterval(timer);
  });

  function visible() {
    if (document.visibilityState !== "visible") return;
    loadList();
    loadThread(true);
  }

  // ---- Writing ----

  function key(event) {
    // Enter sends, Shift+Enter is a new line — the way every chat works.
    if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
      event.preventDefault();
      send();
    }
  }

  function addFiles(list) {
    if (!thread?.attach_to.length) {
      toast("Dere jobber ikke i samme selskap, så det finnes ingen innboks å legge vedlegget i", false);
      return;
    }
    files = [...files, ...list];
  }

  function paste(event) {
    const pasted = [...(event.clipboardData?.files || [])];
    if (pasted.length) {
      event.preventDefault();
      addFiles(pasted);
    }
  }

  // A file already in the inbox from a send that failed later on: a
  // second try must reuse it, or the inbox refuses it as a duplicate.
  const uploaded = new WeakMap();

  async function send() {
    if (sending || (!text.trim() && !files.length)) return;
    sending = true;
    try {
      const documents = [];
      for (const file of files) {
        let id = uploaded.get(file)?.[attachTo];
        if (!id) {
          const name = file.name || "bilde-" + Date.now() + ".png";
          const up = await api("/companies/" + attachTo + "/inbox?reuse_pending=true&filename=" + encodeURIComponent(name), {
            method: "POST",
            headers: { "content-type": file.type || "application/octet-stream" },
            body: file,
          });
          id = up.document_id;
          uploaded.set(file, { ...uploaded.get(file), [attachTo]: id });
        }
        documents.push(id);
      }
      await post("/chat/conversations/" + openId + "/messages", { text, documents });
      text = "";
      files = [];
      await loadThread(false);
      scroller?.scrollTo({ top: scroller.scrollHeight });
      loadList();
      input?.focus();
    } catch (e) {
      toast(e.message, false);
    } finally {
      sending = false;
    }
  }

  // The company names an attachment can go to, from the contact's links.
  function companyName(id) {
    return thread?.with.links.find((l) => l.company_id === id)?.company || "selskapet";
  }
</script>

<svelte:document onvisibilitychange={visible} />

{#if !companyId}
  <div class="navbar bg-base-100 shadow-sm">
    <div class="flex-1 gap-2 items-baseline min-w-0">
      <a href="#/" class="btn btn-ghost text-xl">regnmed</a>
      <span class="text-sm opacity-70 truncate">Meldinger</span>
    </div>
    <div class="flex-none gap-2">
      <UserMenu />
    </div>
  </div>
{/if}

<!-- In the shell the messenger fills what is left of the screen below
     wherever it starts: the navbar, the phone's menu button, the
     breadcrumb and any banner are all above it, and vary. -->
<div
  bind:this={frame}
  style={companyId ? `height: calc(100dvh - ${top}px - 1.5rem)` : undefined}
  class="flex bg-base-100 {companyId
    ? 'min-h-96 rounded-box border border-base-200 overflow-hidden'
    : 'h-[calc(100dvh-4rem)]'}"
>
  <!-- The conversation list. On a phone it is the whole screen until a
       conversation is opened. -->
  <aside
    class="{openId || isNew ? 'hidden ' + pane.wide : 'flex'} w-full {pane.list} shrink-0 flex-col
           border-r border-base-200 min-h-0"
    aria-label="Samtaler"
  >
    <div class="flex items-center gap-2 p-3">
      <h1 class="text-lg font-semibold flex-1">Meldinger</h1>
      <a class="btn btn-sm btn-primary" href={base + "/ny"}>Ny melding</a>
    </div>
    <div class="px-3 pb-2">
      <input
        type="search"
        class="input input-sm w-full"
        placeholder="Søk i samtaler"
        aria-label="Søk i samtaler"
        bind:value={filter}
      />
    </div>
    <div class="flex-1 overflow-y-auto min-h-0">
      {#if listError}
        <div role="alert" class="alert alert-error m-3 text-sm"><span>{listError}</span></div>
      {:else if !conversations}
        <span class="loading loading-spinner loading-md m-4"></span>
      {:else if !conversations.length}
        <div class="p-4 text-sm opacity-80">
          <p class="font-medium">Ingen samtaler ennå.</p>
          <p class="mt-1">
            Skriv til en kollega, regnskapsføreren eller en av selskapets agenter. Legg ved et
            bilde og be agenten bokføre det.
          </p>
          <a class="btn btn-sm mt-3" href={base + "/ny"}>Start en samtale</a>
        </div>
      {:else}
        <ul>
          {#each shown as c (c.conversation_id)}
            <li>
              <a
                href={base + "/" + c.conversation_id}
                class="flex gap-3 items-center px-3 py-2 hover:bg-base-200 focus-visible:bg-base-200
                       {c.conversation_id === openId ? 'bg-base-200' : ''}"
                aria-current={c.conversation_id === openId ? "page" : undefined}
              >
                <Agentansikt
                  navn={c.with.name}
                  farge={c.with.avatar_color}
                  ikon={c.with.avatar_icon}
                  storrelse="w-10"
                />
                <div class="min-w-0 flex-1">
                  <div class="flex items-baseline gap-2">
                    <span class="truncate flex-1 {c.unread ? 'font-semibold' : ''}">{c.with.name}</span>
                    <span class="text-xs opacity-60 shrink-0">{when(c.last_at)}</span>
                  </div>
                  <div class="flex items-center gap-2">
                    <span class="text-sm truncate flex-1 {c.unread ? '' : 'opacity-60'}">
                      {#if c.last_text}{c.last_from_me ? "Du: " : ""}{c.last_text}{:else}{via(c.with)}{/if}
                    </span>
                    {#if c.unread}
                      <span class="badge badge-sm badge-primary" aria-label="{c.unread} uleste">{c.unread}</span>
                    {/if}
                  </div>
                </div>
              </a>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </aside>

  <section class="{openId || isNew ? 'flex' : 'hidden ' + pane.wide} flex-1 flex-col min-w-0 min-h-0">
    {#if isNew}
      <!-- Choosing who to write to. -->
      <div class="flex items-center gap-2 p-3 border-b border-base-200">
        <a class="btn btn-sm btn-ghost {pane.narrow}" href={base} aria-label="Tilbake til samtalene">←</a>
        <h2 class="font-semibold flex-1">Ny melding</h2>
      </div>
      <div class="p-3">
        <input
          bind:this={search}
          type="search"
          class="input w-full"
          placeholder="Hvem vil du skrive til?"
          aria-label="Søk etter person eller agent"
          bind:value={contactFilter}
          onkeydown={(e) => {
            if (e.key === "Enter" && contactsShown.length) start(contactsShown[0]);
            if (e.key === "Escape") location.hash = base;
          }}
        />
      </div>
      <div class="flex-1 overflow-y-auto min-h-0">
        {#if !contacts}
          <span class="loading loading-spinner loading-md m-4"></span>
        {:else if !contacts.length}
          <p class="p-4 text-sm opacity-80">
            Det er ingen du kan skrive til ennå. Kolleger, regnskapsfører og selskapets agenter dukker
            opp her når de har tilgang.
          </p>
        {:else}
          <ul>
            {#each contactsShown as c (c.person_id + (c.company_id || ""))}
              <li>
                <button
                  class="flex gap-3 items-center w-full text-left px-3 py-2 hover:bg-base-200 focus-visible:bg-base-200"
                  onclick={() => start(c)}
                >
                  <Agentansikt navn={c.name} farge={c.avatar_color} ikon={c.avatar_icon} storrelse="w-10" />
                  <div class="min-w-0">
                    <div class="truncate">
                      {c.name}
                      {#if c.agent}<span class="badge badge-xs badge-outline ml-1">agent</span>{/if}
                    </div>
                    <div class="text-sm opacity-60 truncate">{via(c)}</div>
                  </div>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    {:else if openId}
      {#if threadError && !thread}
        <div class="p-3">
          <a class="btn btn-sm btn-ghost {pane.narrow} mb-2" href={base}>← Samtalene</a>
          <div role="alert" class="alert alert-warning">
            <span>
              Fant ikke samtalen. Den kan være avsluttet fordi tilgangen eller oppdraget er avsluttet.
            </span>
          </div>
        </div>
      {:else if !thread}
        <span class="loading loading-spinner loading-md m-4"></span>
      {:else}
        <div class="flex items-center gap-3 p-3 border-b border-base-200">
          <a class="btn btn-sm btn-ghost {pane.narrow}" href={base} aria-label="Tilbake til samtalene">←</a>
          <Agentansikt
            navn={thread.with.name}
            farge={thread.with.avatar_color}
            ikon={thread.with.avatar_icon}
            storrelse="w-9"
          />
          <div class="min-w-0">
            <h2 class="font-semibold truncate">{thread.with.name}</h2>
            <p class="text-xs opacity-60 truncate">{via(thread.with)}</p>
          </div>
        </div>
        {#if thread.runs_here === false}
          <!-- Not ours to run: it answers only if it runs elsewhere (MCP).
               Said up front, so a message does not wait in silence. -->
          <p role="note" class="alert alert-info alert-soft rounded-none text-sm py-2">
            <span>
              {thread.with.name} kjøres utenfor regnmed og svarer bare når den er i gang.
              <a class="link" href={AGENT_DOK} target="_blank" rel="noopener">Les mer</a>
            </span>
          </p>
        {/if}

        <div
          bind:this={scroller}
          onscroll={onScroll}
          class="flex-1 overflow-y-auto min-h-0 p-3 flex flex-col gap-2"
          role="log"
          aria-live="polite"
          aria-label={"Samtale med " + thread.with.name}
        >
          {#if thread.older}
            <!-- Also a button: scrolling is not the only way to read back,
                 and a keyboard user needs a target. -->
            <button class="btn btn-ghost btn-xs self-center" onclick={loadOlder} disabled={lasterEldre}>
              {lasterEldre ? "Henter eldre meldinger …" : "Vis eldre meldinger"}
            </button>
          {/if}
          {#if !thread.messages.length}
            <p class="m-auto text-sm opacity-70 text-center max-w-xs">
              {#if thread.with.agent}
                Skriv hva {thread.with.name} skal gjøre. Legg gjerne ved et bilde av kvitteringen — agenten
                svarer her.
              {:else}
                Start samtalen med {thread.with.name}.
              {/if}
            </p>
          {/if}
          {#each thread.messages as m (m.message_id)}
            {@const mine = m.author_id === thread.me}
            <div class="flex flex-col {mine ? 'items-end' : 'items-start'}">
              <div
                class="rounded-box px-3 py-2 text-sm whitespace-pre-wrap break-words max-w-[85%]
                  {mine ? 'bg-primary text-primary-content' : 'bg-base-200'}"
              >
                {m.text}
              </div>
              {#each m.documents as d (d.document_id)}
                <button
                  class="link text-xs mt-1"
                  onclick={() =>
                    download("/companies/" + d.company_id + "/inbox/" + d.document_id + "/content", d.filename)}
                  >📎 {d.filename}</button
                >
              {/each}
              <span class="text-xs opacity-60 mt-0.5">
                {formatTime(m.created_at)}
                {#if m.task_status}· {thread.with.name} {STATUS[m.task_status] || m.task_status}{/if}
              </span>
            </div>
          {/each}
        </div>

        <div class="border-t border-base-200 p-2 sm:p-3 flex flex-col gap-2">
          {#if files.length}
            <div class="flex flex-wrap items-center gap-2">
              {#each files as f, i (i)}
                <span class="badge badge-outline gap-1">
                  📎 {f.name || "bilde"}
                  <button
                    aria-label="Fjern {f.name || 'bilde'}"
                    onclick={() => (files = files.filter((_, j) => j !== i))}>✕</button
                  >
                </span>
              {/each}
              {#if thread.attach_to.length > 1}
                <label class="text-xs flex items-center gap-1">
                  til innboksen i
                  <select class="select select-xs" bind:value={attachTo}>
                    {#each thread.attach_to as id (id)}
                      <option value={id}>{companyName(id)}</option>
                    {/each}
                  </select>
                </label>
              {:else}
                <span class="text-xs opacity-60">legges i innboksen til {companyName(attachTo)}</span>
              {/if}
            </div>
          {/if}
          <div class="flex gap-2 items-end">
            {#if thread.attach_to.length}
              <input
                bind:this={fileInput}
                type="file"
                multiple
                accept="image/*,application/pdf,.xml"
                class="hidden"
                onchange={(e) => {
                  addFiles([...e.currentTarget.files]);
                  e.currentTarget.value = "";
                }}
              />
              <button
                class="btn btn-ghost btn-square"
                aria-label="Legg ved fil eller bilde"
                onclick={() => fileInput.click()}>📎</button
              >
            {/if}
            <textarea
              bind:this={input}
              bind:value={text}
              class="textarea flex-1 min-h-10 leading-snug"
              rows="1"
              aria-label={"Melding til " + thread.with.name}
              placeholder={"Skriv til " + thread.with.name}
              onkeydown={key}
              onpaste={paste}
            ></textarea>
            <button
              class="btn btn-primary"
              onclick={send}
              disabled={sending || (!text.trim() && !files.length)}
            >
              {#if sending}<span class="loading loading-spinner loading-xs"></span>{/if}
              Send
            </button>
          </div>
        </div>
      {/if}
    {:else}
      <div class="m-auto text-center opacity-80 p-6">
        <p>Velg en samtale, eller start en ny.</p>
        <a class="btn btn-sm mt-3" href={base + "/ny"}>Ny melding</a>
      </div>
    {/if}
  </section>
</div>
