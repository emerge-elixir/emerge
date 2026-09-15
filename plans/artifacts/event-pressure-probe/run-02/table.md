# Event pressure measurements

Ranges across three separate release processes per case.

| Nodes | Input | Offered/s (or burst) | Pause | Channel full | Incoming peak | Listener FIFO peak | Outbox peak | Settled within recovery window |
|---:|---|---:|---|---:|---:|---:|---:|---:|
| 1 | edit | 30 | none | 0 | 1 | 0 | 0 | 3/3 |
| 1 | edit | 125 | none | 0 | 1 | 0 | 0 | 3/3 |
| 1 | edit | 1000 | none | 0 | 1 | 0 | 0 | 3/3 |
| 1 | edit | 8000 | none | 0 | 1–7 | 0–5 | 0 | 3/3 |
| 1 | pointer | 1000 | none | 0 | 1 | 0 | 0 | 3/3 |
| 1 | pointer | 8000 | none | 0 | 1 | 0 | 0 | 3/3 |
| 1 | ime | 30 | none | 0 | 1 | 0 | 0 | 3/3 |
| 1 | ime | 1000 | none | 0 | 1 | 0 | 0 | 3/3 |
| 20,000 | edit | 30 | none | 0 | 1 | 0 | 0 | 3/3 |
| 20,000 | edit | 125 | none | 0 | 1 | 0 | 0 | 3/3 |
| 20,000 | edit | 1000 | none | 0 | 1 | 736–812 | 0 | 0/3 |
| 20,000 | edit | 8000 | none | 0 | 2 | 7770–7799 | 0 | 0/3 |
| 20,000 | pointer | 1000 | none | 0 | 1 | 0 | 0 | 3/3 |
| 20,000 | pointer | 8000 | none | 0 | 1 | 0 | 0 | 3/3 |
| 20,000 | ime | 30 | none | 0 | 1 | 0 | 0 | 3/3 |
| 20,000 | ime | 1000 | none | 0 | 2 | 0 | 0 | 3/3 |
| 1 | edit | 30 | tree | 0 | 1 | 29 | 0 | 3/3 |
| 1 | edit | 1000 | tree | 0 | 1 | 999 | 0 | 3/3 |
| 20,000 | edit | 30 | tree | 0 | 1 | 29 | 0 | 3/3 |
| 20,000 | edit | 1000 | tree | 0 | 1 | 999 | 0 | 0/3 |
| 1 | pointer | 8000 | tree | 0 | 1–3 | 1 | 0 | 3/3 |
| 1 | ime | 1000 | tree | 0 | 1 | 0 | 488 | 3/3 |
| 1 | edit | 30 | event | 0 | 30 | 29 | 0 | 3/3 |
| 1 | edit | 1000 | event | 0 | 1000 | 999 | 0 | 3/3 |
| 1 | edit | 8000 | event | 3904 | 4096 | 4095 | 0 | 3/3 |
| 1 | pointer | 8000 | event | 3904 | 4096 | 0 | 0 | 3/3 |
| 1 | edit | 20000 burst | none | 8145–10044 | 4096 | 9951–11849 | 0 | 3/3 |
| 1 | pointer | 100000 burst | none | 48864–59424 | 4096 | 0 | 0 | 3/3 |
| 1 | ime | 20000 burst | none | 10124–13869 | 4096 | 0 | 0 | 3/3 |
| 20,000 | edit | 20000 burst | none | 8024–9190 | 4096 | 10809–11975 | 0 | 0/3 |
