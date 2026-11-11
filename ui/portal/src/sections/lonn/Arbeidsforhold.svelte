<script module>
  // Arbeidsforholdet slik a-meldingen krever det (F290/F291, migrasjon
  // 0073). Skjemaet holder tekst; tilPayload gjør det om til det API-et
  // tar. Tomt felt = ikke oppgitt, aldri en standardverdi.
  export function tomt() {
    return {
      virksomhet: "",
      yrkeskode: "",
      stillingsprosent: "",
      timer: "",
      arbeidstidsordning: "",
      ansettelsesform: "",
      sisteLonnsendring: "",
    };
  }

  const hundredeler = (n) => (n == null ? "" : String(n / 100).replace(".", ","));

  export function fraAnsatt(a) {
    const f = a.arbeidsforhold || {};
    return {
      virksomhet: f.virksomhet_orgnr || "",
      yrkeskode: f.yrkeskode || "",
      stillingsprosent: hundredeler(f.stillingsprosent_bp),
      timer: hundredeler(f.timer_per_uke_hundredeler),
      arbeidstidsordning: f.arbeidstidsordning || "",
      ansettelsesform: f.ansettelsesform || "",
      sisteLonnsendring: f.siste_lonnsendring || "",
    };
  }

  const tall = (s) => (s.trim() ? Math.round(parseFloat(s.trim().replace(",", ".")) * 100) : null);
  const tekst = (s) => s.trim() || null;

  export function tilPayload(v) {
    return {
      virksomhet_orgnr: tekst(v.virksomhet.replace(/\s/g, "")),
      yrkeskode: tekst(v.yrkeskode),
      stillingsprosent_bp: tall(v.stillingsprosent),
      timer_per_uke_hundredeler: tall(v.timer),
      arbeidstidsordning: tekst(v.arbeidstidsordning),
      ansettelsesform: tekst(v.ansettelsesform),
      siste_lonnsendring: tekst(v.sisteLonnsendring),
    };
  }

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
  let { value = $bindable(), koder } = $props();
</script>

<div class="grid gap-2">
  <div class="grid grid-cols-2 gap-2">
    <input
      class="input input-sm"
      placeholder="Virksomhet (underenhetens orgnr)"
      title="Underenheten i Enhetsregisteret der den ansatte jobber — ikke selskapets eget organisasjonsnummer"
      bind:value={value.virksomhet}
    />
    <input
      class="input input-sm"
      placeholder="Yrkeskode (7 siffer)"
      title="Yrkeskode fra SSBs yrkeskatalog (STYRK-08), 7 siffer"
      bind:value={value.yrkeskode}
    />
  </div>
  <div class="grid grid-cols-2 gap-2">
    <input
      class="input input-sm"
      placeholder="Stillingsprosent (f.eks. 100)"
      bind:value={value.stillingsprosent}
    />
    <input
      class="input input-sm"
      placeholder="Timer/uke i full stilling (f.eks. 37,5)"
      bind:value={value.timer}
    />
  </div>
  <div class="grid grid-cols-2 gap-2">
    <select class="select select-sm" bind:value={value.arbeidstidsordning}>
      <option value="">— arbeidstidsordning —</option>
      {#each koder?.arbeidstidsordning || [] as kode (kode)}
        <option value={kode}>{navn(kode)}</option>
      {/each}
    </select>
    <select class="select select-sm" bind:value={value.ansettelsesform}>
      <option value="">— ansettelsesform —</option>
      {#each koder?.ansettelsesform || [] as kode (kode)}
        <option value={kode}>{navn(kode)}</option>
      {/each}
    </select>
  </div>
  <label class="text-xs opacity-70 flex items-center gap-2">
    Siste lønnsendring (nyansatt: startdatoen)
    <input type="date" class="input input-sm w-auto" bind:value={value.sisteLonnsendring} />
  </label>
</div>
