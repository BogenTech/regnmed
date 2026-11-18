---
title: API-referanse
description: Oversikt over alle endepunktene, gruppert etter område, med rettigheten hvert endepunkt krever.
sidebar:
  order: 0
---

Referansen er delt i områder. Hver side lister metode, sti og rettigheten handleren krever, generert fra rutetabellen i koden. Forespørsels- og svarformer for de mest brukte kallene står i [Eksempler](/developers/examples/). Hvert område peker til det tekniske dokumentet som forklarer reglene bak.

| Område | Omfang |
| --- | --- |
| [Identitet og tilgang](/developers/api/identity/) | `/me`, medlemmer, roller, invitasjoner, byrå og oppdrag, abonnement, vilkår |
| [Selskap og hovedbok](/developers/api/ledger/) | Onboarding, kontoplan, bilag, vedlegg, periodelås, dimensjoner, årsavslutning |
| [Salg og reskontro](/developers/api/sales/) | Faktura, tilbud, ordre, produkter, kunder, åpne poster |
| [Regnskap og rapporter](/developers/api/reports/) | Spesifikasjoner, resultat, balanse, mva, SAF-T, revisjon, budsjett |
| [Bank og betaling](/developers/api/bank/) | Avstemming, OCR, betalingslister, kort, valuta |
| [Lønn, timer og utlegg](/developers/api/payroll/) | Ansatte, lønn, a-melding, timer, utlegg |
| [Innboks, import og jobber](/developers/api/inbox/) | Bilagsinnboks, e-post-inn, SAF-T og CSV-import |
| [Aksjeeierbok og anlegg](/developers/api/shares/) | Aksjonærer, utbytte, RF-1086, anleggsregister |
| [Integrasjoner, agenter og meldinger](/developers/api/agents/) | Maskintilgang, agentoppgaver, samtaler |
| [Forankring og verifisering](/developers/api/verification/) | Åpen feed, bevis per selskap |
| [Plattform](/developers/api/platform/) | Leverandørens konsoll (ikke integrasjons-API) |

## Felles regler

- Autentisering: `Authorization: Bearer <token>`. Se [Autentisering](/developers/authentication/).
- Alle selskapsendepunkter ligger under `/companies/{company_id}/…`. Uten tilgang er svaret `404`.
- Beløp i øre, datoer `YYYY-MM-DD`, feil som `{"error": "…"}`. Se [Konvensjoner](/developers/conventions/).
- Endepunkter som har en egen rettighet for «egne» og «alle» (`_EGNE` og `_ALLE`) lar `_ALLE` medføre `_EGNE`.

## Utenfor referansen

| Sti | Hvorfor |
| --- | --- |
| `/`, `/callback`, `/assets/*`, `/ny`, `/portal-config`, `/manifest.webmanifest`, `/sw.js`, ikoner | Betjener nettportalen |
| `/auth/token`, `/auth/refresh`, `/auth/logout`, `/auth/device`, `/auth/device/poll` | Portalens innlogging, ikke for integrasjoner |
| `/webhooks/stripe`, `/webhooks/altinn` | Innkommende kall fra Stripe og Altinn, signert av dem. Ikke for deg å kalle |

Den lengre, håndskrevne oversikten i repoet ([docs/api.md](https://github.com/BogenTech/regnmed/blob/main/docs/api.md)) henger noe etter rutetabellen, og referansen her er generert fra koden.
