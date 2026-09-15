| Nodes | Owners / loops¹ | Case | Warm p50 / p95 (ms) | Release median [min–max] (ms) | Settle median (ms) | Warm RSS MiB |
|---:|---:|---|---:|---:|---:|---:|
| 20000 | 64 | paint | 1.670 / 1.849 | 1.708 [1.647–2.554] | — | 165.7 |
| 20000 | 64 | pixel | 2.121 / 2.348 | 2.107 [1.994–2.138] | — | 165.5 |
| 20000 | 64 | length | 2.422 / 2.604 | 15.888 [14.522–16.952] | — | 316.6 |
| 20000 | 64 | moving | 2.989 / 3.437 | 15.600 [15.292–16.513] | — | 316.4 |
| 20000 | 64 | mixed | 3.978 / 4.561 | 5.164 [4.656–6.116] | 29.455 | 317.0 |
| 20000 | 64 | upward | 71.172 / 86.106 | 76.988 [75.213–96.760] | 32.373 | 378.7 |

¹ `upward` counts looping children under one finite Content parent. Other cases count finite owners.
Warm columns are medians of per-process quantiles, not pooled distributions. RSS is not exact live heap.
