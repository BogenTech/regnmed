<script>
  import { theme, THEMES, ICON, setTheme, cycleTheme } from "../lib/theme.svelte.js";
</script>

<select
  class="select select-sm select-bordered w-32"
  value={theme.current}
  onchange={(e) => setTheme(e.currentTarget.value)}
  aria-label="Velg fargetema"
>
  {#each THEMES as t}
    <option value={t}>{t}</option>
  {/each}
</select>
<button
  class="btn btn-ghost btn-sm text-lg"
  onclick={cycleTheme}
  title={"Fargetema: " + theme.current + " (klikk for å bytte)"}
  aria-label={"Fargetema: " + theme.current + " (klikk for å bytte)"}
>
  {ICON[theme.current] || "🎨"}
</button>
