window.BENCHMARK_DATA = {
  "lastUpdate": 1788740205205,
  "repoUrl": "https://github.com/sims1253/ry",
  "entries": {
    "ry performance": [
      {
        "commit": {
          "author": {
            "email": "dev.scholz@mailbox.org",
            "name": "Maximilian Scholz",
            "username": "sims1253"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "99b62493c719927ee76f383d7988f2005bac9f0f",
          "message": "Merge pull request #242 from sims1253/t3code/add-performance-regression-ci\n\nci: track core performance and extension activation",
          "timestamp": "2026-09-07T01:08:34+02:00",
          "tree_id": "603e3e31285bd8cbef451da536fa9f2f6f26ac93",
          "url": "https://github.com/sims1253/ry/commit/99b62493c719927ee76f383d7988f2005bac9f0f"
        },
        "date": 1788736452714,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 11335241.962642519,
            "range": "11321202.46–11349713.12",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 1829276.189813473,
            "range": "1824010.81–1836182.78",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 11267437.777590487,
            "range": "11222803.82–11314967.76",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 5709465.397421714,
            "range": "5698669.84–5721151.54",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1212806.1628142328,
            "range": "1210793.22–1214975.53",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2211861.9215387567,
            "range": "2208486.31–2215405.50",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 760166.5110046621,
            "range": "755975.73–764879.29",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 5697215.97722504,
            "range": "5684514.94–5712481.63",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1196269.8707881044,
            "range": "1194483.10–1198306.67",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 46141575.6125,
            "range": "45630018.34–46645753.22",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 10304377.183497632,
            "range": "10175165.77–10480785.00",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 4734031.937261907,
            "range": "4726231.97–4741345.76",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 10188111.786350086,
            "range": "10138730.91–10242989.77",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 3922766.8316316395,
            "range": "3903890.23–3942623.69",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 8237399.595174594,
            "range": "8213451.34–8264131.29",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 11950392,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 808535,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 122655,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 101.59931700001471,
            "range": "85.99–110.97",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 85.98566599999322, 94.94961600005627, 101.59931700001471, 110.80865500000073, 110.9745759999496"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 420.7609639999573,
            "range": "415.91–446.12",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 415.90583800000604, 419.8109939999995, 420.7609639999573, 431.51472399994964, 446.11940000002505"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "dev.scholz@mailbox.org",
            "name": "Maximilian Scholz",
            "username": "sims1253"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "e67fb5586700871754b735eb6295cd25af08d82e",
          "message": "Merge pull request #243 from sims1253/docs/concise-start\n\ndocs: add facts export to usage guide",
          "timestamp": "2026-09-07T01:18:21+02:00",
          "tree_id": "f89aa54d29ba8375a50b81e9a01572dde95231c3",
          "url": "https://github.com/sims1253/ry/commit/e67fb5586700871754b735eb6295cd25af08d82e"
        },
        "date": 1788737009270,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 11010404.621015664,
            "range": "10983398.82–11042925.12",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 1770230.7338065826,
            "range": "1764095.61–1777555.64",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 10654820.788182048,
            "range": "10633969.13–10678977.46",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 5534260.456597831,
            "range": "5507551.25–5582278.15",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1171575.500201465,
            "range": "1165802.02–1178212.53",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2148226.9481892353,
            "range": "2144247.81–2153000.51",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 728008.5564657571,
            "range": "726667.08–729969.86",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 5636527.105528312,
            "range": "5556362.59–5749451.65",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1149947.6466603342,
            "range": "1148144.79–1152150.76",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 43158612.7875,
            "range": "42438044.95–44157460.24",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 9940209.32389709,
            "range": "9809965.96–10136347.17",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 4638810.14795904,
            "range": "4615748.27–4666273.16",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 9819451.108574923,
            "range": "9763003.76–9889073.59",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 3765531.894122188,
            "range": "3747051.38–3784287.67",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 8178363.398261217,
            "range": "8002039.16–8392546.51",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 11950392,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 808535,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 122655,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 102.37310100000468,
            "range": "74.36–121.85",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 74.35723900000448, 89.19796000001952, 102.37310100000468, 118.49219000001904, 121.85353100000066"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 399.83233199999086,
            "range": "353.42–560.45",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 353.4193920000107, 391.87793000001693, 399.83233199999086, 476.6223089999985, 560.4546479999844"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "dev.scholz@mailbox.org",
            "name": "Maximilian Scholz",
            "username": "sims1253"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "26654ceb4ccc9f4245408e0f5f38233bcdae3fdf",
          "message": "Merge pull request #245 from sims1253/fix/assignment-type-hints\n\nfix(lsp): show inferred types at each assignment",
          "timestamp": "2026-09-07T01:26:42+02:00",
          "tree_id": "7c63ade62336545324f0e4e217b6bf514b6f1d48",
          "url": "https://github.com/sims1253/ry/commit/26654ceb4ccc9f4245408e0f5f38233bcdae3fdf"
        },
        "date": 1788737561828,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 13305159.047587607,
            "range": "13248873.34–13368575.43",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2174226.8366733002,
            "range": "2171691.45–2177860.31",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 13630387.096768266,
            "range": "13545221.22–13727520.57",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7114843.474024857,
            "range": "7109633.77–7121205.56",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1442920.222091238,
            "range": "1441286.69–1444835.39",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2958231.596446157,
            "range": "2951679.81–2966205.46",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 993874.8929152681,
            "range": "982656.26–1014284.00",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7076983.690987354,
            "range": "7069727.99–7085494.56",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1424344.4115540679,
            "range": "1422459.41–1426968.91",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 60802732.58333333,
            "range": "60401597.39–61202276.37",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12060186.075945452,
            "range": "12035906.13–12096784.70",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5778379.765826245,
            "range": "5663590.35–5992761.67",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12287311.045366494,
            "range": "12216681.73–12381568.09",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4622795.64069237,
            "range": "4616517.44–4629353.82",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 10250335.340806639,
            "range": "10209728.10–10295600.20",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 11972864,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 808535,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 122727,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 142.3945450000465,
            "range": "124.15–166.46",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 124.14715199999046, 130.89719399996102, 142.3945450000465, 146.489103000029, 166.45652100001462"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 495.456666000071,
            "range": "465.60–521.09",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 465.6001379999798, 467.18729699996766, 495.456666000071, 496.52954199991655, 521.0906960000284"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "dev.scholz@mailbox.org",
            "name": "Maximilian Scholz",
            "username": "sims1253"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "6e307e3265f21387906323071d11049553354ca3",
          "message": "Merge pull request #244 from sims1253/fix/opaque-ops-lookup\n\nfix(checker): stop S3 lookup at opaque Ops methods",
          "timestamp": "2026-09-07T01:36:27+02:00",
          "tree_id": "56a228fe12dcedd8bd9b9d6a27ce2fce37c0c948",
          "url": "https://github.com/sims1253/ry/commit/6e307e3265f21387906323071d11049553354ca3"
        },
        "date": 1788738043490,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 13330687.949013393,
            "range": "13250652.68–13419909.53",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2181351.8455400653,
            "range": "2179000.54–2184299.21",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 13709020.80453366,
            "range": "13666497.91–13763196.86",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7138256.121122966,
            "range": "7126394.74–7153408.64",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1441193.2539969042,
            "range": "1438368.67–1444648.71",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2937670.8154344102,
            "range": "2925049.67–2952489.30",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 1001416.430507412,
            "range": "987661.75–1026541.95",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7112846.629029008,
            "range": "7105996.00–7120556.66",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1427096.6537995702,
            "range": "1423733.49–1431973.81",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 61597705.133333325,
            "range": "61245672.56–61951390.58",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12232488.607128378,
            "range": "12203949.37–12268849.40",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5725445.3676163815,
            "range": "5679550.02–5806934.57",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12340114.300732542,
            "range": "12263366.09–12445300.71",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4697861.856136032,
            "range": "4683444.82–4714453.05",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 10180502.916432587,
            "range": "10162998.52–10202003.56",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 11973280,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 808535,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 122727,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 137.2846530000097,
            "range": "120.30–140.42",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 120.30290000000969, 124.39960000000428, 137.2846530000097, 138.71145299999625, 140.41615500001353"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 466.98133900001994,
            "range": "453.03–567.14",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 453.0323710000084, 463.52189999999246, 466.98133900001994, 469.2786560000095, 567.1383300000452"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "dev.scholz@mailbox.org",
            "name": "Maximilian Scholz",
            "username": "sims1253"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "c954c95e8915d2d0b2e7d7dc2d64206b3d0657d5",
          "message": "Merge pull request #248 from sims1253/fix/code-action-eligibility\n\nfix(lsp): respect code action eligibility and kinds",
          "timestamp": "2026-09-07T01:48:33+02:00",
          "tree_id": "e2f7f8cf4158941c340ecce369842750c7175e73",
          "url": "https://github.com/sims1253/ry/commit/c954c95e8915d2d0b2e7d7dc2d64206b3d0657d5"
        },
        "date": 1788738767188,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 14633452.765697619,
            "range": "14350009.16–14923105.60",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2220943.766709106,
            "range": "2218852.38–2223216.03",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14040087.483624209,
            "range": "13974769.96–14116149.86",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6793896.545593603,
            "range": "6775961.84–6813083.67",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1409825.5284149223,
            "range": "1406085.23–1414329.54",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2908328.612676927,
            "range": "2898286.51–2924757.30",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 993227.2580007624,
            "range": "985626.10–1004654.58",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6765461.85195346,
            "range": "6748047.66–6784196.99",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1401918.3183512532,
            "range": "1398418.77–1406063.80",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 66807233.78333334,
            "range": "66606661.93–67004718.11",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12522353.260770924,
            "range": "12496566.88–12550778.97",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5648793.207603681,
            "range": "5642125.97–5655985.96",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12480444.626675503,
            "range": "12466690.73–12495031.69",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4804018.096781908,
            "range": "4779802.22–4835807.31",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 10470836.741798837,
            "range": "10438577.00–10506310.39",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 11973904,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 808535,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 122727,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 139.5347039999906,
            "range": "134.93–156.29",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 134.9325190000236, 138.93384200002765, 139.5347039999906, 141.46855900000082, 156.28954700002214"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 497.61205300001893,
            "range": "485.02–559.66",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 485.01704500004416, 489.11917899997206, 497.61205300001893, 524.7974500000128, 559.6632090000203"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "dev.scholz@mailbox.org",
            "name": "Maximilian Scholz",
            "username": "sims1253"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "0395eea23a52a7f8fd41c91b1f65ca3a08a5134a",
          "message": "Merge pull request #251 from sims1253/fix/versioned-suppression-edits\n\nfix(lsp): version suppression edits for capable clients",
          "timestamp": "2026-09-07T01:49:01+02:00",
          "tree_id": "510afe5a586d93c10f8f3d35857b4ae7142966be",
          "url": "https://github.com/sims1253/ry/commit/0395eea23a52a7f8fd41c91b1f65ca3a08a5134a"
        },
        "date": 1788739023678,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 19293235.301515777,
            "range": "18847492.36–19782487.91",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2203054.2029498327,
            "range": "2199560.61–2207296.18",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14254507.455261346,
            "range": "14052838.71–14551724.63",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6686777.200794562,
            "range": "6678459.25–6695418.46",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1394959.626724311,
            "range": "1393105.40–1397311.83",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2902116.1905433745,
            "range": "2895229.62–2912097.27",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 978105.4003491635,
            "range": "973921.65–983208.57",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6739271.591605234,
            "range": "6712782.35–6773316.93",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1394787.6114688597,
            "range": "1393147.67–1396800.30",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 69380332.51666668,
            "range": "69040054.74–69729428.75",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12606420.789296817,
            "range": "12526104.49–12703802.36",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5751040.876945629,
            "range": "5722430.22–5790983.31",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12549472.048358781,
            "range": "12499122.37–12612157.82",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4798851.883953745,
            "range": "4785977.31–4814277.53",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 10635810.572973305,
            "range": "10578504.08–10709378.95",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 11980296,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 808535,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 122727,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 128.1507059999858,
            "range": "116.83–134.44",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 116.82672099999036, 120.5088650000107, 128.1507059999858, 130.30764199999976, 134.44054200002574"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 486.20516099999077,
            "range": "460.20–495.61",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 460.1969799999497, 478.8881199999887, 486.20516099999077, 487.2222619999957, 495.6095149999892"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "dev.scholz@mailbox.org",
            "name": "Maximilian Scholz",
            "username": "sims1253"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "ff1f5baaee6a3acb538bda17006c81f476be8b80",
          "message": "Merge pull request #252 from sims1253/fix/stale-diagnostic-actions\n\nReject suppression requests with stale diagnostics",
          "timestamp": "2026-09-07T01:54:04+02:00",
          "tree_id": "ce4d53f4c98269255864eed07c1ab135edf21b03",
          "url": "https://github.com/sims1253/ry/commit/ff1f5baaee6a3acb538bda17006c81f476be8b80"
        },
        "date": 1788739298759,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 16327422.734455476,
            "range": "15640723.01–17040306.83",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2180651.5845293617,
            "range": "2173742.12–2189459.66",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 13994443.146244396,
            "range": "13905813.16–14098947.36",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7191969.343580022,
            "range": "7168720.84–7228906.79",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1452781.191561446,
            "range": "1442465.98–1469007.69",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2918556.8275801735,
            "range": "2913488.15–2926653.24",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 992810.3537648652,
            "range": "985929.56–1002577.86",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7109944.608866541,
            "range": "7104268.71–7114908.73",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1430575.0005431627,
            "range": "1427186.81–1434197.77",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 69406758.6,
            "range": "68867390.97–69943076.96",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12455449.925239198,
            "range": "12411674.68–12501155.10",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5716391.354488353,
            "range": "5703075.63–5731320.70",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12474077.433393652,
            "range": "12373065.91–12579703.22",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4675327.5211164,
            "range": "4657320.35–4696817.33",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 10217768.017858315,
            "range": "10179507.26–10262254.43",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 11988008,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 808535,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 122727,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 144.00377499999013,
            "range": "121.89–156.60",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 121.89470699999947, 137.4139960000175, 144.00377499999013, 155.05967899999814, 156.59510199999204"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 496.51237499999115,
            "range": "471.13–547.06",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 471.1349119999795, 475.1806010000291, 496.51237499999115, 506.47255499998573, 547.0578080000123"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "dev.scholz@mailbox.org",
            "name": "Maximilian Scholz",
            "username": "sims1253"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "977e3469309ec48af227eb66949767b2b534ff28",
          "message": "Merge pull request #249 from sims1253/fix/callback-invalidation\n\nfix(checker): invalidate incremental callback readers",
          "timestamp": "2026-09-07T02:06:42+02:00",
          "tree_id": "cf5c1a2d52049c2faeba1a1af4a8f7b1cfb9bcdf",
          "url": "https://github.com/sims1253/ry/commit/977e3469309ec48af227eb66949767b2b534ff28"
        },
        "date": 1788739932257,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 12573183.038309587,
            "range": "12495409.44–12638902.02",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2036591.2682581642,
            "range": "2031023.75–2042206.44",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 12247272.915862577,
            "range": "12189052.98–12303317.59",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6471967.820508597,
            "range": "6437655.71–6506107.19",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1374066.4818213475,
            "range": "1363337.47–1385032.04",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2506686.5445910804,
            "range": "2497948.06–2516028.79",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 846590.9664641805,
            "range": "842177.78–850493.03",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6419837.955169686,
            "range": "6400682.20–6438456.03",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1362875.7312658434,
            "range": "1358490.02–1368038.30",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 50019596.583333336,
            "range": "49536916.99–50553427.79",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 11493273.196749574,
            "range": "11440230.40–11561856.50",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5385426.571910852,
            "range": "5338612.14–5438487.98",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 11459083.467349853,
            "range": "11416975.89–11496853.53",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4376978.012242116,
            "range": "4332669.72–4441013.23",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 13848137.972067585,
            "range": "13758688.99–13970882.63",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 11997024,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 808535,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 122727,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 105.11401299998397,
            "range": "99.37–133.92",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 99.36985399998957, 103.68834100000095, 105.11401299998397, 115.34384200000204, 133.92015799996443"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 468.4453039999935,
            "range": "401.83–477.97",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 401.8316019999911, 459.3424690000247, 468.4453039999935, 469.5414710000041, 477.96985799999675"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "dev.scholz@mailbox.org",
            "name": "Maximilian Scholz",
            "username": "sims1253"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "90229998e48a3b7647596fe50ac7d8c6bceadb58",
          "message": "Merge pull request #253 from sims1253/feat/ordered-reference-prefixes\n\nfeat(facts): preserve ordered writes and proven prefixes",
          "timestamp": "2026-09-07T02:09:45+02:00",
          "tree_id": "d1211259cf6f7a9a22a42da5833b242f0d4a936c",
          "url": "https://github.com/sims1253/ry/commit/90229998e48a3b7647596fe50ac7d8c6bceadb58"
        },
        "date": 1788740205122,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 14048720.033287201,
            "range": "13887641.49–14227104.86",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2217005.1576960026,
            "range": "2207082.44–2228663.50",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14007397.16750079,
            "range": "13949693.92–14069768.95",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7294015.155304563,
            "range": "7278380.77–7311043.53",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1443022.8122729745,
            "range": "1442211.37–1443954.11",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2992566.0297654243,
            "range": "2974489.30–3025766.08",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 984040.6276540287,
            "range": "982413.72–986030.24",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7187117.2030489715,
            "range": "7173319.84–7203420.30",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1433066.7910109668,
            "range": "1431336.36–1435185.51",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 67014620.383333325,
            "range": "66387998.24–67884244.12",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12928587.689495053,
            "range": "12879188.95–12978266.49",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5778781.129740877,
            "range": "5753296.00–5814168.39",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12350782.688720308,
            "range": "12303927.03–12403009.78",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4733380.392454094,
            "range": "4717320.46–4750192.23",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15890620.518497083,
            "range": "15714573.51–16125375.77",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12001152,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 808535,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 122727,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 143.84058600000571,
            "range": "117.65–158.37",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 117.64659899997059, 137.98288800002774, 143.84058600000571, 155.37825000000885, 158.36995600000955"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 506.1937190000317,
            "range": "439.80–579.71",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 439.79789399995934, 454.772564999992, 506.1937190000317, 516.1226219999953, 579.71454299998"
          }
        ]
      }
    ]
  }
}