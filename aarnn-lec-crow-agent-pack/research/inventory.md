# Robot inventory and design availability

Research date: 23 October 2026. “Listed” means advertised in LEC’s public catalogue or rental line-up; it does not establish LEC’s owned inventory or serial-number-level configuration.

The homepage lists 12 families. Its rental page adds Galbot G1, DINERBOT T11 and COFE+ Smart Bar, giving **15 commercial families across six manufacturer/brand groups**. UGOT contributes seven distinct forms: mecanum, quadruped, four-legged spider, self-balancing car, wheeled-and-legged robot, engineer vehicle and transforming car. This produces **21 LEC configurations plus the requested crow: 22 new profiles**. Six existing AARNN profiles are included for the MuJoCo/Isaac Sim expansion, for **28 total profiles**.

| Brand / manufacturer | LEC model / additional animal | Type | Design result | Evidence |
|---|---|---|---|---|
| KEENON | DINERBOT T8 | tray delivery | No verified public mechanical model found; specification-led surrogate required. | [keenon-t8](https://www.keenon.com/en/product/T8); [lec-dinerbot-t8](https://www.lecrobotics.ai/products/dinerbot-t8) |
| KEENON | DINERBOT T9 | tray delivery | No verified public mechanical model found; specification-led surrogate required. | [keenon-t9](https://www.keenon.com/en/product/T9); [lec-dinerbot-t9](https://www.lecrobotics.ai/products/dinerbot-t9) |
| KEENON | DINERBOT T10 | tray delivery advertising | No verified public mechanical model found; specification-led surrogate required. | [keenon-t10](https://www.keenon.com/en/product/T10); [lec-dinerbot-t10](https://www.lecrobotics.ai/products/dinerbot-t10) |
| KEENON | DINERBOT T11 | narrow aisle delivery advertising | No verified public mechanical model found; specification-led surrogate required. | [keenon-t11](https://www.keenon.com/en/product/T11); [lec-rentals](https://www.lecrobotics.ai/rentals) |
| KEENON | BUTLERBOT W3 | enclosed delivery | No verified public mechanical model found; specification-led surrogate required. | [keenon-w3](https://www.keenon.com/en/product/W3); [lec-butlerbot-w3](https://www.lecrobotics.ai/products/butlerbot-w3) |
| KEENON | COURIER S100 | heavy load delivery | No verified public mechanical model found; specification-led surrogate required. | [keenon-s100](https://www.keenon.com/en/product/S100); [lec-courier-s100](https://www.lecrobotics.ai/products/courier-s100) |
| KEENON | KLEENBOT C30 | dry floor cleaner | No verified public mechanical model found; specification-led surrogate required. | [keenon-c30](https://www.keenon.com/en/product/C30); [lec-kleenbot-c30](https://www.lecrobotics.ai/products/kleenbot-c30) |
| KEENON | KLEENBOT C40 | wet dry floor cleaner | No verified public mechanical model found; specification-led surrogate required. | [keenon-c40](https://www.keenon.com/en/product/C40); [lec-kleenbot-c40](https://www.lecrobotics.ai/products/kleenbot-c40) |
| BOTINKIT | OMNI | automated induction wok | No verified public mechanical model found; specification-led surrogate required. | [lec-botinkit-omni-cooking-robot](https://www.lecrobotics.ai/products/botinkit-omni-cooking-robot) |
| COFE+ / Shanghai Hi-Dolphin | COFE+ Robot Café | robotic coffee kiosk | No verified public mechanical model found; specification-led surrogate required. | [lec-cofe-plus-robot-cafe](https://www.lecrobotics.ai/products/cofe-plus-robot-cafe) |
| COFE+ / Shanghai Hi-Dolphin | COFE+ Smart Bar | robotic coffee bar | No verified public mechanical model found; specification-led surrogate required. | [lec-rentals](https://www.lecrobotics.ai/rentals) |
| XBOT / Yingzhi (InSpace) | XBOT S Pro | dual arm robotic barista | No verified public mechanical model found; specification-led surrogate required. | [lec-xbot-s-pro](https://www.lecrobotics.ai/products/xbot-s-pro) |
| XBOT / Yingzhi (InSpace) | XBOT IC / XBOT-IC60 | robotic ice cream kiosk | No verified public mechanical model found; specification-led surrogate required. | [lec-xbot-ic](https://www.lecrobotics.ai/products/xbot-ic) |
| UBTECH | UGOT: all seven configurations | Modular wheeled / legged educational kit | Only the mecanum form has a bundled community MIT MJCF/USD; no verified mechanical designs found for the other six forms. | [ugot-docs](https://docs.ubtrobot.com/ugot/); [lec-ugot](https://www.lecrobotics.ai/products/ugot) |
| Galbot / Galaxy General Robotics | Galbot G1 (official Golf description) | wheeled dual arm mobile manipulator | Official Apache-2.0 URDF + MJCF + USD + meshes bundled; LEC variant unconfirmed. | [galbot-description](https://github.com/GalaxyGeneralRobotics/galbot_one_golf_description/tree/2d496b053f0d4e9e2688f59fac66022f447226be); [lec-rentals](https://www.lecrobotics.ai/rentals) |
| Biological analogue | Carrion crow (Corvus corone) | biological bird | CC0/CC BY visuals identified; physical crow model must be authored. | [crow-rspb](https://www.rspb.org.uk/birds-and-wildlife/identifying-birds/corvids-how-to-identify-the-crow-family); [crow-bto](https://www.bto.org/learn/about-birds/birdfacts/carrion-crow) |

## Model identity and specification conflicts

- **Galbot G1:** the official developer platform links the One Golf description as its G1 asset. This is a wheeled dual-arm mobile manipulator. LEC does not specify its hardware subvariant; confirm the head, grippers/suction tools and torso before calling an import an exact replica of LEC’s unit. Do not replace it with Unitree G1.
- **XBOT S Pro:** LEC’s [webpage](https://www.lecrobotics.ai/products/xbot-s-pro) gives approx. W2.200 × D1.700 × H2.170 m and 900 kg. Its linked S PRO brochure gives W1.130 × D1.280 × H1.600 m and 400 kg. These are incompatible descriptions, not alternative unit conversions. Keep separate selectable surrogate variants until the actual unit is identified. No age or generation is inferred from the conflict.
- **COFE+ Smart Bar:** the rental name identifies a separate coffee-bar offering. The manufacturer’s current Robot Coffee Bar page is a research lead; its current generation must not silently become LEC’s exact Smart Bar configuration.
- **S100:** LEC’s webpage/brochure gives 925 × 620 × 1282 mm; KEENON’s current page gives 925 × 640 × 1282 mm (W × D × H). Model both provenance records and label the selected footprint.
- **C40:** manufacturer body W578 × D500 × H690 mm excludes the squeegee; W616 × D550 × H690 mm includes it. This is a real accessory/configuration distinction.
- **DINERBOT tables:** LEC’s web comparison places kg values under an obstacle-detection heading. Those cells are not sensor ranges. Use the manufacturer dimensions and the PDF’s correctly labelled fields.
- **T11 payload:** the manufacturer states 20 kg total but the parenthetical middle/bottom figures sum to 15 kg. Keep tray-specific remaining allocation unresolved.

No public verified complete mechanical description was found for the remaining commercial products or six other UGOT forms in the searches performed. This is a bounded search result, not proof that no files exist. An OEM SDK, support manual, patent illustration or public photograph does not supply missing inertias, joint limits, motor curves, sensor extrinsics or redistribution rights.

The crow is **Corvus corone**, distinct from a raven or jackdaw. Identified crow meshes are visual aids; no complete validated crow musculoskeletal or connectome package is claimed.

All source URLs, research notes and brochure digests are in `sources.json`. Machine-readable parameters and conflicts are in `../manifests/robots.json`.
