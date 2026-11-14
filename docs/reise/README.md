# Statens kostsatser i utlandet

`kostsatser-utland-2026.csv` er vedlegget til **Særavtale om dekning av
utgifter til reise og kost utenfor Norge**, «Satser for kostgodtgjørelse
gjeldende fra 1. januar 2026», hentet 2026-11-13 fra

<https://www.regjeringen.no/no/dokumenter/saravtale-om-dekning-av-utgifter-til-reise-og-kost-utenfor-norge/id3082991/>

(Word-versjonen, `…/no/word/saeravtale-reise-utenfor-norge.docx`,
SHA-256 `e4fd171338d575de10db2b6daa802103d37bad07f037f82e732053d87a0d3f49`).
Tabellen er lest rett ut av dokumentets tabellceller, ikke tastet inn.
Departementet publiserer den ikke maskinlesbart.

Kolonnene er tabellens egne: region, land, ISO-kode, by eller område,
kostsats og valuta. Tre ting er lagt til, og står i `merknad`:

- ISO-kode for Afghanistan, Jemen og Nord-Korea, som tabellen lar stå
  tom.
- Departementets administrativt fastsatte land («Andorra: som for
  Spania», …) som egne rader med satsen de viser til. For Storbritannia
  er det satsen utenom London.

Et land med bare én rad (Sverige: Stockholm) bruker den raden for hele
landet. «Afrika øvrige» og de andre regionsradene gjelder land som ikke
står i tabellen.

Migrasjon 0087 lagrer radene i `travel_rate_abroad` (datert,
innsettings-bar). Et nytt år er nye rader med ny `valid_from`, hentet
og kontrollert på samme måte — punkt på den årlige
regelverksrevisjonen. Hvordan satsene brukes står i
[docs/utlegg.md «Reise og diett»](../utlegg.md).
