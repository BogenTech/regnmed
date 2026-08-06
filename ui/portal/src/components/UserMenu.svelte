<script>
  // The user menu (2026-08-06): one avatar dropdown in every logged-in
  // navbar — who you are, where identity is edited (the IdP's own
  // /account page, never here), the theme picker, logout, and which
  // version of the system answered (/portal-config, baked in by CI).
  //
  // The theme picker inside is the same theme-controller machinery as
  // the old standalone ThemeControls (which the login page still uses):
  // checked radios pick the theme in pure CSS, the JS only remembers the
  // choice and pre-applies it before first paint. See ThemeControls for
  // the full reasoning, including why «Følg systemet» is NOT a
  // controller.
  import { me } from "../lib/me.svelte.js";
  import { session, logout } from "../lib/auth.svelte.js";
  import { theme, THEME_GROUPS, ICON, setTheme } from "../lib/theme.svelte.js";

  // Optional: where this context's settings live (the company console's
  // Administrasjon). Hosts without a natural settings page omit it.
  let { innstillingerHref = null } = $props();

  let initialer = $derived(
    (me.name || "?")
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((del) => del[0].toUpperCase())
      .join("") || "?",
  );

  let profilUrl = $derived(
    session.config?.issuer ? session.config.issuer.replace(/\/$/, "") + "/account" : null,
  );

  function velgTema(t) {
    setTheme(t);
  }
</script>

<div class="dropdown dropdown-end">
  <div
    tabindex="0"
    role="button"
    class="btn btn-ghost btn-circle avatar avatar-placeholder"
    aria-label={"Brukermeny for " + (me.name || "bruker")}
  >
    <div class="bg-primary text-primary-content w-9 rounded-full">
      <span class="text-sm font-semibold">{initialer}</span>
    </div>
  </div>
  <ul
    class="dropdown-content menu menu-sm bg-base-200 rounded-box z-20 mt-2 w-64 p-2 shadow-lg"
  >
    <li class="menu-title">
      <span class="text-base-content font-semibold">{me.name || "Innlogget"}</span>
      {#if me.email}
        <span class="font-normal normal-case">{me.email}</span>
      {/if}
    </li>
    {#if profilUrl}
      <li>
        <a href={profilUrl} target="_blank" rel="noreferrer">
          Profil
          <span class="text-xs opacity-60">hos innloggingstjenesten ↗</span>
        </a>
      </li>
    {/if}
    {#if innstillingerHref}
      <li><a href={innstillingerHref}>Innstillinger</a></li>
    {/if}
    <li>
      <details>
        <summary>Fargetema <span class="opacity-60">{ICON[theme.current] || "🎨"}</span></summary>
        <ul class="max-h-72 flex-nowrap overflow-y-auto">
          {#each THEME_GROUPS as gruppe (gruppe.label)}
            <li class="menu-title">{gruppe.label}</li>
            {#each gruppe.themes as t (t)}
              <li>
                <input
                  type="radio"
                  name="tema"
                  class="btn btn-xs btn-ghost btn-block justify-start
                         {t === 'system' ? '' : 'theme-controller'}"
                  aria-label={t}
                  value={t}
                  checked={theme.current === t}
                  onchange={() => velgTema(t)}
                />
              </li>
            {/each}
          {/each}
        </ul>
      </details>
    </li>
    <div class="divider my-1"></div>
    <li><button onclick={logout}>Logg ut</button></li>
    <li class="menu-title pt-1">
      <span class="font-normal text-xs opacity-60">
        regnmed {session.config?.versjon || "dev"}
      </span>
    </li>
  </ul>
</div>
