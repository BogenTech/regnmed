<script>
  // Meldinger in every navbar, with the unread count: the assistant and
  // the accountant are one click away from wherever you are.
  import { chat } from "../lib/chat.svelte.js";
  import { prefs } from "../lib/prefs.svelte.js";
  import { route } from "../lib/router.svelte.js";
  import Ikon from "./Ikon.svelte";

  // Inside a company, stay inside it: the messenger opens in its shell.
  const href = $derived(
    route.view === "company" ? "#/c/" + route.companyId + "/meldinger" : "#/meldinger",
  );
</script>

<a
  {href}
  class="btn btn-ghost btn-circle indicator"
  aria-label={chat.unread > 0 ? "Meldinger, " + chat.unread + " uleste" : "Meldinger"}
  title="Meldinger"
>
  {#if chat.unread > 0}
    <span class="indicator-item badge badge-xs badge-primary">{chat.unread}</span>
  {/if}
  <!-- A button with no icon would be an empty circle: "ingen ikoner"
       keeps this one, drawn as a line icon. -->
  <Ikon navn="meldinger" stil={prefs.ikonstil === "ingen" ? "linje" : null} />
</a>
