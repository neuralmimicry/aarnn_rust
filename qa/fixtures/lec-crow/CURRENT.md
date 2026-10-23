# Current expansion evidence

Generated 2026-10-23T16:34:03.917848+00:00. Full profile/target acceptance is incomplete. Passing subchecks are narrow imports/reference loops, not trained tasks or governed output. Historical evidence remains retained: `source_matches_worktree` and `changed_source_paths` identify source differences; a historical pass does not certify changed code.

224 rows: blocked=56, fail=24, not_run=144.

| Profile | webots | unity | unreal | webgl | minecraft_java | minecraft_bedrock | mujoco | isaacsim |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| keenon_t8 | not_run | blocked | fail | not_run | not_run | not_run | not_run | blocked |
| keenon_t9 | not_run | blocked | fail | not_run | not_run | not_run | not_run | blocked |
| keenon_t10 | not_run | blocked | fail | not_run | not_run | not_run | not_run | blocked |
| keenon_t11 | not_run | blocked | fail | not_run | not_run | not_run | not_run | blocked |
| keenon_w3 | not_run | blocked | fail | not_run | not_run | not_run | not_run | blocked |
| keenon_s100 | not_run | blocked | fail | not_run | not_run | not_run | not_run | blocked |
| keenon_c30 | not_run | blocked | fail | not_run | not_run | not_run | not_run | blocked |
| keenon_c40 | not_run | blocked | fail | not_run | not_run | not_run | not_run | blocked |
| botinkit_omni | not_run | blocked | not_run | not_run | not_run | not_run | not_run | blocked |
| cofe_robot_cafe | not_run | blocked | not_run | not_run | not_run | not_run | not_run | blocked |
| cofe_smart_bar | not_run | blocked | not_run | not_run | not_run | not_run | not_run | blocked |
| xbot_s_pro | not_run | blocked | not_run | not_run | not_run | not_run | not_run | blocked |
| xbot_ic60 | not_run | blocked | not_run | not_run | not_run | not_run | not_run | blocked |
| ugot_mecanum | not_run | blocked | fail | not_run | not_run | not_run | not_run | blocked |
| ugot_quadruped | not_run | blocked | fail | not_run | not_run | not_run | not_run | blocked |
| ugot_spider | not_run | blocked | fail | not_run | not_run | not_run | not_run | blocked |
| ugot_balance | not_run | blocked | fail | not_run | not_run | not_run | not_run | blocked |
| ugot_wheelleg | not_run | blocked | fail | not_run | not_run | not_run | not_run | blocked |
| ugot_engineer | not_run | blocked | fail | not_run | not_run | not_run | not_run | blocked |
| ugot_transform | not_run | blocked | fail | not_run | not_run | not_run | not_run | blocked |
| galbot_g1_golf | not_run | blocked | fail | not_run | not_run | not_run | not_run | blocked |
| crow_corvus_corone | not_run | blocked | fail | not_run | not_run | not_run | not_run | blocked |
| celegans | not_run | blocked | fail | not_run | not_run | not_run | not_run | blocked |
| drosophila_banc | not_run | blocked | not_run | not_run | not_run | fail | fail | blocked |
| drosophila_fafb | not_run | blocked | not_run | not_run | not_run | not_run | fail | blocked |
| hexapod | not_run | blocked | fail | not_run | not_run | not_run | not_run | blocked |
| nao | not_run | blocked | fail | not_run | not_run | not_run | not_run | blocked |
| zebrafish | not_run | blocked | not_run | not_run | not_run | not_run | fail | blocked |

Detailed model/IO hashes, subchecks and evidence: [acceptance-matrix.current.json](acceptance-matrix.current.json).

Latest lane results:

- `capabilities`: **pass**, `target/qa/lec-crow/capabilities-pigzcnmz/result.json`, SHA-256 `ca4b4298d9705b4a9dc6ed96981390168463c8c2302efbcc588458356408fc29`.
- `contract`: **pass**, `target/qa/lec-crow/contract-jl59upqe/result.json`, SHA-256 `b163be8f35dd8918ae8e7691c488f4ca20cab19d26fa9c750fb145fe8218100f`.
- `fleet`: **fail**, `target/qa/lec-crow/fleet-mvrkmcrn/result.json`, SHA-256 `7470512dbd581ae9f713b620cf8a59b0bd7af5cd663074756014783ad85a829d`.
- `habitats`: **pass**, `target/qa/lec-crow/habitats-bszteajo/result.json`, SHA-256 `afab85f28fdccb38909a1806c5120bfe9b9f77e497c57a76bf858a25930dc586`.
- `launchers`: **pass**, `target/qa/lec-crow/launchers-g1n0iavq/result.json`, SHA-256 `99b693d9df7ae1094ce4328d342c65e1c34d7ba0de81c0861ac1ff5f4a8b12cc`.
- `legacy-neural`: **fail**, `target/qa/lec-crow/legacy-neural-n7zg3gcc/result.json`, SHA-256 `3748b9b0e92ef9f9d9eac76358a32bacd61faa41b709bd1fa3c1fa83dc4d5655`.
- `minecraft-service`: **pass**, `target/qa/lec-crow/minecraft-service-ttkmuqcf/result.json`, SHA-256 `2e5a285c34e55cd57d51fa51e2c805d0802fea882a9f0e8ee91eb5eef17a13c3`.
- `mujoco-managed`: **pass**, `target/qa/lec-crow/mujoco-managed-6ref4cqa/result.json`, SHA-256 `03e6d45448986154e21079a8b91c65d79f6ca370671f204900063539e2f754c8`.
- `mujoco-population`: **pass**, `target/qa/lec-crow/mujoco-population-kma0wd1c/result.json`, SHA-256 `3deb5c1db8c5c998aad1ee7d54fdfa88fd48d60375f6a3ffea15459937de5e39`.
- `mujoco-viewer`: **pass**, `target/qa/lec-crow/mujoco-viewer-u0xgv_wt/result.json`, SHA-256 `1d14b9793c4f45d985db986f3c69ebd264c24ea2931ad2b4f54d448d51499dd5`.
- `neural`: **pass**, `target/qa/lec-crow/neural-hx6t6rd8/result.json`, SHA-256 `2b6ef9430edbd0223d046965921f58950770c16161051fc790452a3b9f4635c1`.
- `npc`: **pass**, `target/qa/lec-crow/npc-ftay8qu1/result.json`, SHA-256 `62c78e1799409391dc2e82fff30d87a8141f4d6e704bae4acaaaf3fd26cacbc2`.
- `rust`: **pass**, `target/qa/lec-crow/rust-vbjfge73/result.json`, SHA-256 `4252a031e2fa05f4a7533fa239fa29d578dd92c997d2a3301bdd6765adb05c22`.
- `rust-neural`: **pass**, `target/qa/lec-crow/rust-neural-pgys3_7l/result.json`, SHA-256 `bad563d243189602b8385b70ce28bd268470d39b706e6d20f63beb83c8017a23`.
- `rust-service`: **pass**, `target/qa/lec-crow/rust-service-fn_xmqf9/result.json`, SHA-256 `f557d05bb8716308646a586b2442f5d555aba9c1e8bdc8d5887a8b0a99442f8c`.
- `shared`: **pass**, `target/qa/lec-crow/shared-3pj_z0zh/result.json`, SHA-256 `bfe667aa6fc9f13a2bb3937722f50b61f8069fc819ddd828ffc76413b04cb5b4`.
- `unreal`: **pass**, `target/qa/lec-crow/unreal-kagtqttq/result.json`, SHA-256 `b3f2d1ae75d0b4be390c652a6150e46f5bb4fb16b3df85dcbfdecac000630d14`.
- `unreal-lifecycle`: **pass**, `target/qa/lec-crow/unreal-lifecycle-kj_v7u_l/result.json`, SHA-256 `ba774f09ef9a2b8b7ee818b1d55c6e4e4d9633070128ab69d12c1513b84b6a63`.
- `unreal-neural`: **pass**, `target/qa/lec-crow/unreal-neural-y5zfdasi/result.json`, SHA-256 `41f977a1e2d49ef5396518abe40b8e6aa22e84e417e8355e0a4b4562c30d8fd4`.
- `unreal-passive`: **fail**, `target/qa/lec-crow/unreal-passive-wrv8sjrz/result.json`, SHA-256 `70045e12346aebd11dc49ac111f9cd567b16c1c572d7d65e1911904d7a0b08a6`.
- `unreal-render`: **pass**, `target/qa/lec-crow/unreal-render-5o8t_i49/result.json`, SHA-256 `c3d641587eb78694e33e0879ebd8fcb4b9cba55e46767a0ca0b43398806c90cd`.
- `usd`: **pass**, `target/qa/lec-crow/usd-mxormckh/result.json`, SHA-256 `b2d1e70d63c31bfa51395e10047881fd513431f1ad04fe2c4193ba80a9bb4658`.
- `webots`: **pass**, `target/qa/lec-crow/webots-sfmzmi99/result.json`, SHA-256 `d910555b3393de55a68e4aeab84811e995eb5ec9ea189048e0d679ee1b5e6cdb`.
- `webots-neural`: **pass**, `target/qa/lec-crow/webots-neural-q8vfarpv/result.json`, SHA-256 `3960f787ec31170a337065e62ca9caddf7dbd10eba5eb575523d62cc1d0c3585`.
- `preserved-bedrock-native`: **fail**, `target/qa/minecraft/bedrock-native-tqx5_icc/result.json`, SHA-256 `13e01afc9b2a19fd9a3d15223adc6e2736a26a9fd6d52200cf2165479ed93f46`.
- `arm64-native`: **pass**, `target/qa/lec-crow/arm64-current/result.json`, SHA-256 `d048b390fd2bc3d9579df7814f73fa3894ea022f9ec9d48e7435450d26226e84`.
