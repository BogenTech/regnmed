<script>
  // Temavelgeren bruker daisyUIs egen theme-controller: en avkrysset
  // radio med temanavnet som value velger temaet i REN CSS
  // (`:root:has(input.theme-controller[value=x]:checked)`).
  //
  // JS-en består likevel, og det er med vilje: theme-controller virker
  // bare på siden som er oppe, mens vi må huske valget (localStorage) og
  // sette det FØR første maling (inline-skriptet i index.html), ellers
  // blinker siden i feil tema. De to sier alltid det samme, siden
  // setTheme kalles av samme klikk som krysser av radioen.
  //
  // «Følg systemet» er den ene som IKKE er en theme-controller: den
  // ligger i samme radiogruppe, så den krysser av de andre — og uten
  // avkrysset controller finnes ingen CSS-overstyring, slik at
  // OS-preferansen får bestemme. (Det måtte være denne veien: en
  // avkrysset controller har HØYERE spesifisitet enn [data-theme], så en
  // gjenglemt radio ville overstyrt «system» i det stille.)
  import { theme, THEME_GROUPS, ICON, setTheme, cycleTheme } from "../lib/theme.svelte.js";

  function velg(t) {
    setTheme(t);
    // daisyUI-nedtrekket lukkes ved blur; uten dette blir det stående
    // åpent etter valget.
    document.activeElement?.blur();
  }
</script>

<div class="dropdown dropdown-end">
  <div
    tabindex="0"
    role="button"
    class="btn btn-ghost btn-sm gap-1"
    aria-label={"Fargetema: " + theme.current}
  >
    <span class="text-lg leading-none">{ICON[theme.current] || "🎨"}</span>
    <span class="hidden sm:inline font-normal">{theme.current}</span>
    <span class="opacity-60">▾</span>
  </div>
  <ul
    class="dropdown-content menu menu-sm bg-base-200 rounded-box z-10 mt-1 w-52
           max-h-96 flex-nowrap overflow-y-auto shadow-lg"
  >
    {#each THEME_GROUPS as gruppe (gruppe.label)}
      <li class="menu-title">{gruppe.label}</li>
      {#each gruppe.themes as t (t)}
        <li>
          <input
            type="radio"
            name="tema"
            class="btn btn-sm btn-ghost btn-block justify-start
                   {t === 'system' ? '' : 'theme-controller'}"
            aria-label={t}
            value={t}
            checked={theme.current === t}
            onchange={() => velg(t)}
          />
        </li>
      {/each}
    {/each}
  </ul>
</div>
<button
  class="btn btn-ghost btn-sm"
  onclick={cycleTheme}
  title={"Fargetema: " + theme.current + " (klikk for å bytte)"}
  aria-label={"Fargetema: " + theme.current + " (klikk for å bytte)"}
>
  ↻
</button>
