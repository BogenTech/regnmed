<script module>
  // Kodene kommer fra serveren (samme liste som a-meldingen sjekkes mot);
  // dette er bare visningsnavnene. En kode uten navn her vises som koden.
  const NAVN = {
    ikkeSkift: "Ikke skift",
    andreSkift: "Andre skiftordninger",
    doegnkontinuerligSkiftOgTurnus355: "Døgnkontinuerlig skift og turnus (35,5 t)",
    helkontinuerligSkiftOgAndreOrdninger336: "Helkontinuerlig skift og andre ordninger (33,6 t)",
    offshore336: "Offshore (33,6 t)",
    skift365: "Skift (36,5 t)",
    fast: "Fast",
    midlertidig: "Midlertidig",
    fastAnsattUtleid: "Fast ansatt for utleie",
    midlertidigAnsattUtleid: "Midlertidig ansatt for utleie",
    midlertidigAnsattSomTilkallingsvikar: "Midlertidig, tilkallingsvikar",
  };
  export const navn = (kode) => NAVN[kode] || kode;
</script>

<script>
  import OccupationPicker from "./OccupationPicker.svelte";

  let { value = $bindable(), koder, companyId, units = [], onunitinput } = $props();
  const uid = $props.id();
  const unit = $derived(units.find((e) => e.orgnumber === value.virksomhet.replace(/\s/g, "")));
</script>

<div class="grid gap-2">
  <div class="grid grid-cols-2 gap-2">
    <label class="flex flex-col gap-1 text-xs min-w-0">
      Virksomhet (underenhetens orgnr)
      <input
        class="input input-sm"
        inputmode="numeric"
        list={uid + "-units"}
        title="Underenheten i Enhetsregisteret der den ansatte jobber — ikke selskapets eget organisasjonsnummer"
        bind:value={value.virksomhet}
        oninput={onunitinput}
      />
      <datalist id={uid + "-units"}>
        {#each units as e (e.orgnumber)}
          <option value={e.orgnumber} label={e.name + (e.municipality ? ", " + e.municipality : "")}></option>
        {/each}
      </datalist>
      {#if unit}
        <span class="opacity-70 truncate">{unit.name}{unit.municipality ? ", " + unit.municipality : ""}</span>
      {/if}
    </label>
    <div class="flex flex-col gap-1 text-xs min-w-0">
      <label for={uid + "-occupation"}>Yrkeskode (STYRK-08)</label>
      <OccupationPicker id={uid + "-occupation"} {companyId} bind:value={value.yrkeskode} />
    </div>
  </div>
  <div class="grid grid-cols-2 gap-2">
    <label class="flex flex-col gap-1 text-xs">
      Stillingsprosent
      <input
        class="input input-sm"
        inputmode="decimal"
        placeholder="f.eks. 100"
        bind:value={value.stillingsprosent}
      />
    </label>
    <label class="flex flex-col gap-1 text-xs">
      Timer per uke i full stilling
      <input
        class="input input-sm"
        inputmode="decimal"
        placeholder="f.eks. 37,5"
        bind:value={value.timer}
      />
    </label>
  </div>
  <div class="grid grid-cols-2 gap-2">
    <label class="flex flex-col gap-1 text-xs">
      Arbeidstidsordning
      <select class="select select-sm" bind:value={value.arbeidstidsordning}>
        <option value="">Velg …</option>
        {#each koder?.arbeidstidsordning || [] as kode (kode)}
          <option value={kode}>{navn(kode)}</option>
        {/each}
      </select>
    </label>
    <label class="flex flex-col gap-1 text-xs">
      Ansettelsesform
      <select class="select select-sm" bind:value={value.ansettelsesform}>
        <option value="">Velg …</option>
        {#each koder?.ansettelsesform || [] as kode (kode)}
          <option value={kode}>{navn(kode)}</option>
        {/each}
      </select>
    </label>
  </div>
  <label class="flex flex-col gap-1 text-xs">
    Siste lønnsendring (nyansatt: startdatoen)
    <input type="date" class="input input-sm w-auto" bind:value={value.sisteLonnsendring} />
  </label>
</div>
