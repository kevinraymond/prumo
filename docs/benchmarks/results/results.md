Machine: 13th Gen Intel(R) Core(TM) i9-13900K, pinned to 1 CPU; pdfTeX 3.141592653-2.6-1.40.29 (TeX Live 2026); image prumo-dev:tl2026-full. Warm medians of 5 runs (3 for the large document); the cold run has a fresh HOME.

### minimal

| phase (ms) | pdflatex | xelatex | lualatex |
|---|---:|---:|---:|
| startup | 41.9 | 82.8 | 235.2 |
| preamble | 7.8 | 45.9 | 21.9 |
| begin-document | 1.6 | 1.9 | 2.0 |
| typesetting | 0.1 | 0.1 | 0.1 |
| shipout | 45.1 | 0.2 | 35.4 |
| end-document | 0.6 | 0.5 | 0.6 |
| finalization | 38.0 | 135.5 | 19.0 |
| **total (wall)** | **134.9** | **267.4** | **314.9** |
| spread (min–max) | 134–136 | 265–269 | 311–316 |
| cold run (wall) | 134.9 | 3,188.5 | 4,416.3 |
| pages | 1 | 1 | 1 |

### standard

| phase (ms) | pdflatex | xelatex | lualatex |
|---|---:|---:|---:|
| startup | 42.3 | 81.7 | 236.1 |
| preamble | 501.9 | 715.2 | 762.8 |
| begin-document | 49.0 | 46.8 | 52.3 |
| typesetting | 148.3 | 266.2 | 202.8 |
| shipout | 59.6 | 19.1 | 63.5 |
| end-document | 2.3 | 3.5 | 3.3 |
| finalization | 51.6 | 56.4 | 71.1 |
| **total (wall)** | **854.7** | **1,191.0** | **1,391.5** |
| spread (min–max) | 852–864 | 1,162–1,209 | 1,374–1,418 |
| cold run (wall) | 859.2 | 4,122.3 | 5,744.9 |
| pages | 5 | 5 | 5 |

### florilegium

| phase (ms) | lualatex |
|---|---:|
| startup | 238.5 |
| preamble | 274.2 |
| begin-document | 12.1 |
| typesetting | 508.7 |
| shipout | 470.9 |
| end-document | 2.1 |
| finalization | 91.3 |
| **total (wall)** | **1,597.4** |
| spread (min–max) | 1,595–1,602 |
| cold run (wall) | 6,015.2 |
| pages | 35 |

### large

| phase (ms) | pdflatex | xelatex | lualatex |
|---|---:|---:|---:|
| startup | 42.3 | 82.5 | 236.3 |
| preamble | 183.6 | 279.7 | 276.7 |
| begin-document | 56.7 | 47.0 | 76.5 |
| typesetting | 13,594.2 | 15,694.2 | 27,220.9 |
| shipout | 2,781.5 | 3,778.4 | 10,933.5 |
| end-document | 23.0 | 21.4 | 24.7 |
| finalization | 111.1 | 227.4 | 157.7 |
| **total (wall)** | **16,793.5** | **20,136.1** | **38,914.9** |
| spread (min–max) | 16,757–16,801 | 20,130–20,142 | 38,899–38,944 |
| cold run (wall) | 16,395.2 | 23,068.1 | 43,350.8 |
| pages | 3991 | 3991 | 3991 |

pdflatex: 8,669 records (164 KiB); `prumo time --json` on the cached run 3 ms; `--html` 8 ms, 1,279 KiB.

xelatex: 8,661 records (165 KiB); `prumo time --json` on the cached run 3 ms; `--html` 7 ms, 1,277 KiB.

lualatex: 8,668 records (169 KiB); `prumo time --json` on the cached run 3 ms; `--html` 7 ms, 1,279 KiB.
