window.BENCHMARK_DATA = {
  "lastUpdate": 1788973545028,
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
          "id": "52227d9c837dc23b4b5e9e7be1feec70487b310a",
          "message": "Merge pull request #254 from sims1253/docs/scope-journal-experiment\n\ndocs: record scope journal experiment tradeoffs",
          "timestamp": "2026-09-07T02:13:59+02:00",
          "tree_id": "c502b25fcb0f81605b8f3757c77f27999a48872f",
          "url": "https://github.com/sims1253/ry/commit/52227d9c837dc23b4b5e9e7be1feec70487b310a"
        },
        "date": 1788740466750,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 15636125.592318207,
            "range": "15288140.34–16008657.61",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2204321.144827275,
            "range": "2199355.03–2210305.82",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 13809901.672697386,
            "range": "13672652.87–14039697.04",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7270722.292285867,
            "range": "7246364.85–7307664.79",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1472230.0354433898,
            "range": "1468516.33–1476538.11",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2968076.257010204,
            "range": "2961186.64–2977956.58",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 996191.9209301518,
            "range": "991378.73–1002823.27",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7281005.052299961,
            "range": "7253101.50–7315935.48",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1441757.4610093664,
            "range": "1439197.95–1444972.08",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 64381181.31666668,
            "range": "63877788.84–65023583.48",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12718213.137907023,
            "range": "12663551.99–12777195.11",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5869366.793063561,
            "range": "5774945.49–5993955.93",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12294212.725235235,
            "range": "12257972.17–12337864.18",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4719053.007212159,
            "range": "4698599.20–4742474.56",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15588472.734011184,
            "range": "15393214.72–15818823.65",
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
            "value": 142.28485799999908,
            "range": "135.94–154.87",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 135.93929099990055, 140.8371460000053, 142.28485799999908, 144.395414999919, 154.8653790000826"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 490.5643520001322,
            "range": "458.18–520.99",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 458.1828000000678, 479.6688449999783, 490.5643520001322, 495.94288500002585, 520.9942429999355"
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
          "id": "57c187ac42c095a060d23574b74d783912c935af",
          "message": "Merge pull request #250 from sims1253/feat/force-contract\n\nUse explicit forcing contracts for lazy-default diagnostics",
          "timestamp": "2026-09-07T02:43:27+02:00",
          "tree_id": "a5a3b0b471cb8502d86aee112ad352aac325fa58",
          "url": "https://github.com/sims1253/ry/commit/57c187ac42c095a060d23574b74d783912c935af"
        },
        "date": 1788742065523,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 12740352.495532274,
            "range": "12671143.54–12835258.82",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2071188.847511536,
            "range": "2068775.99–2073891.19",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 12366088.08184324,
            "range": "12172420.01–12718316.05",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6214293.714323435,
            "range": "6206014.70–6224524.01",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1330970.339511244,
            "range": "1326810.69–1336431.96",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2624432.784731829,
            "range": "2620280.62–2630804.48",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 911999.3445106925,
            "range": "906693.91–918380.35",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6188458.433344139,
            "range": "6176883.50–6203630.15",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1327995.296165703,
            "range": "1322494.31–1336430.88",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 62597177.15,
            "range": "62381000.70–62818558.99",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 11376367.295650948,
            "range": "11337216.83–11417042.37",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5501710.366158364,
            "range": "5465308.44–5554613.48",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 11360769.246965494,
            "range": "11319089.22–11405863.69",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4338629.837003559,
            "range": "4320332.33–4365300.19",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 13804471.39017899,
            "range": "13773257.35–13841767.25",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12020440,
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
            "value": 127.34448499995051,
            "range": "109.04–162.69",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 109.04322300001513, 124.61126499995589, 127.34448499995051, 137.81486600002972, 162.6929329999839"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 476.6987889999873,
            "range": "448.63–520.44",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 448.62788599997293, 456.4225030000089, 476.6987889999873, 511.129374000011, 520.4368590000086"
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
          "id": "cff01d277b074a9ddebda70a4c8ef2585a259c8b",
          "message": "Merge pull request #255 from sims1253/fix/ledger-package-counts\n\nfix(ci): validate corpus package diagnostic totals",
          "timestamp": "2026-09-07T02:44:09+02:00",
          "tree_id": "f1f81170f0d8ca8ac171ff8337d1cb982e0277b9",
          "url": "https://github.com/sims1253/ry/commit/cff01d277b074a9ddebda70a4c8ef2585a259c8b"
        },
        "date": 1788742327043,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 13518217.41980118,
            "range": "13496871.43–13544158.64",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2213933.6242885175,
            "range": "2211922.35–2215871.93",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 13946046.762108047,
            "range": "13918990.13–13978037.20",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6754047.134198408,
            "range": "6732739.68–6780768.40",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1403013.6359066071,
            "range": "1400506.17–1406089.38",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2917256.8315313766,
            "range": "2900670.87–2937487.44",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 988558.2910290305,
            "range": "977046.38–1010106.76",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6715092.076619634,
            "range": "6707561.93–6722811.04",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1401970.6932701585,
            "range": "1399997.50–1404830.48",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 65683170.81666668,
            "range": "65295664.16–66066531.54",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12531617.566620806,
            "range": "12502785.62–12566976.96",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5753725.754995676,
            "range": "5743839.53–5767044.97",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12713928.162722398,
            "range": "12673293.66–12751257.92",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4897209.895701439,
            "range": "4852832.83–4952088.37",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15556054.295375522,
            "range": "15395614.93–15773549.60",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12020440,
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
            "value": 133.08223300002282,
            "range": "114.87–167.09",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 114.86534499999834, 127.61851800000295, 133.08223300002282, 145.3667719999794, 167.08536300004926"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 491.6082569999853,
            "range": "442.07–510.10",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 442.0713739999919, 471.4058129999903, 491.6082569999853, 504.76130600000033, 510.09802900004433"
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
          "id": "c5857b06249efc5def3c6b1ec5e544c1b9529d02",
          "message": "Merge pull request #256 from sims1253/perf/function-refinement-dependencies\n\nperf(checker): retain per-function refinement dependencies",
          "timestamp": "2026-09-07T03:06:34+02:00",
          "tree_id": "822bf9d8a2192339af2480174618381a74595724",
          "url": "https://github.com/sims1253/ry/commit/c5857b06249efc5def3c6b1ec5e544c1b9529d02"
        },
        "date": 1788743461144,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 14430234.16334802,
            "range": "14335676.50–14537179.92",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2221769.434538657,
            "range": "2217585.65–2226849.00",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14007604.671607202,
            "range": "13970256.84–14055083.30",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6845271.237611299,
            "range": "6831526.71–6863192.22",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1401533.6279542998,
            "range": "1399519.94–1403969.47",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2956593.4688017084,
            "range": "2943362.40–2975582.20",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 990133.2235780921,
            "range": "983255.22–1002081.57",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6899294.732834274,
            "range": "6851972.64–6970800.01",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1403695.4904246498,
            "range": "1402157.09–1405514.02",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 72690681.94999999,
            "range": "70805731.01–75077606.62",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12618855.715254677,
            "range": "12600734.53–12639924.91",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5759296.857929329,
            "range": "5749036.91–5771232.56",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12669825.65240784,
            "range": "12634309.59–12711485.96",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4862491.530005843,
            "range": "4842106.53–4887695.59",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15833794.057624519,
            "range": "15665016.37–16071767.35",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1135612.3296579397,
            "range": "1127544.52–1143059.97",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12043792,
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
            "value": 123.20587900001556,
            "range": "119.94–166.67",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 119.93668699997943, 120.7786709999782, 123.20587900001556, 159.82954700000118, 166.66553300002124"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 475.1059959999984,
            "range": "451.59–531.67",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 451.5901759999688, 458.6533749999944, 475.1059959999984, 506.4697729999898, 531.6660780000093"
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
          "id": "15041041b56f84e02207389d1e82f59a143739a1",
          "message": "Merge pull request #258 from sims1253/fix/semantic-comment-nodes\n\nfix(parser): keep comments out of executable expressions",
          "timestamp": "2026-09-07T03:08:32+02:00",
          "tree_id": "e9fb6f594b4bd3f12ceb8c1eff4e3b59770a2269",
          "url": "https://github.com/sims1253/ry/commit/15041041b56f84e02207389d1e82f59a143739a1"
        },
        "date": 1788743719266,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 13685462.556064809,
            "range": "13628336.30–13772716.45",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2227629.7002445967,
            "range": "2217861.36–2244738.43",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14174656.76029188,
            "range": "14033655.97–14432982.67",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6697228.4744505575,
            "range": "6688908.48–6710118.37",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1393503.7615529406,
            "range": "1391477.42–1396086.30",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2883944.4993637563,
            "range": "2880654.36–2887997.89",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 984304.1176733412,
            "range": "981660.19–988474.02",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6681822.4365995275,
            "range": "6674675.27–6689069.73",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1390971.3491548712,
            "range": "1387587.00–1396353.75",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 62718284.78333332,
            "range": "62626540.41–62809917.32",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12630522.862356463,
            "range": "12621854.84–12640509.60",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5631665.361749208,
            "range": "5625919.22–5637600.07",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12627586.214006063,
            "range": "12603170.37–12651133.26",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4970415.55757906,
            "range": "4953717.41–4989106.83",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15954998.846283618,
            "range": "15797580.50–16176013.43",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1156829.0390211144,
            "range": "1130501.03–1186533.18",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12044464,
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
            "value": 119.30457100004423,
            "range": "108.54–160.37",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 108.54045000000042, 118.16738600001554, 119.30457100004423, 146.20163800002774, 160.36877900001127"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 496.6595650000381,
            "range": "454.29–539.31",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 454.2855410000193, 478.1092129999888, 496.6595650000381, 497.72349800000666, 539.3099740000034"
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
          "id": "780a43710d163045ffe159705bccd58598825990",
          "message": "Merge pull request #257 from sims1253/fix/structure-constructor\n\nfix(checker): respect base structure argument binding",
          "timestamp": "2026-09-07T03:39:14+02:00",
          "tree_id": "f29ac07466595255374e2e589c4ad63e262291d6",
          "url": "https://github.com/sims1253/ry/commit/780a43710d163045ffe159705bccd58598825990"
        },
        "date": 1788745433730,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 13446364.296110231,
            "range": "13150819.91–13794745.20",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2162073.6686717216,
            "range": "2158909.28–2165604.95",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 13663150.062561885,
            "range": "13619222.79–13720804.90",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7252538.1793942135,
            "range": "7232135.02–7278549.17",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1470585.6493567908,
            "range": "1468800.31–1472637.50",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 3049617.858259478,
            "range": "2986294.91–3156351.88",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 1006020.8028905779,
            "range": "992074.75–1032279.55",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7216055.41764279,
            "range": "7177226.02–7285200.54",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1463720.5230479136,
            "range": "1462179.32–1465397.36",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 59828080.750000015,
            "range": "59143972.21–60607275.05",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12313506.492705273,
            "range": "12247730.93–12406954.31",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5687426.449965889,
            "range": "5673797.70–5705653.99",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12405000.771815594,
            "range": "12350893.22–12472509.53",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4809744.274807757,
            "range": "4741605.46–4891969.12",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15383787.22381953,
            "range": "15209906.52–15628461.74",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1155375.870415757,
            "range": "1151791.13–1158879.07",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12060128,
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
            "value": 142.83257699996466,
            "range": "126.07–163.36",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 126.07287299999734, 138.2211890000035, 142.83257699996466, 147.28551300003892, 163.36189100000774"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 474.93267899996135,
            "range": "445.44–514.73",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 445.43690400000196, 460.0897119999863, 474.93267899996135, 481.0629429999972, 514.7296029999852"
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
          "id": "354c70018970b7bc39ce812c9bec249185b92b4c",
          "message": "Merge pull request #263 from sims1253/docs/scope-journal-followups\n\ndocs: record further scope-journal experiments",
          "timestamp": "2026-09-07T03:40:25+02:00",
          "tree_id": "54cc3e3df4b1c162547e56498b138b6df666240e",
          "url": "https://github.com/sims1253/ry/commit/354c70018970b7bc39ce812c9bec249185b92b4c"
        },
        "date": 1788745683677,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 10700176.496137783,
            "range": "10644067.47–10775134.32",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 1696673.6907102,
            "range": "1695282.59–1698095.09",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 10173142.053729633,
            "range": "10105169.06–10250620.82",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 5260155.210010146,
            "range": "5257422.30–5263141.55",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1110782.507460877,
            "range": "1106553.50–1116257.05",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2272058.5425470667,
            "range": "2268999.03–2275384.24",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 757332.4822118816,
            "range": "754154.71–762884.47",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 5293687.030633042,
            "range": "5285610.02–5302310.17",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1105648.8998330724,
            "range": "1104488.78–1106929.21",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 53817719.63333334,
            "range": "53177529.26–54506999.04",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 9221570.932359684,
            "range": "9206258.15–9237148.05",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 4408393.63511621,
            "range": "4395322.49–4423376.66",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 9203746.184418047,
            "range": "9186837.60–9219274.64",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 3633186.573098057,
            "range": "3570483.92–3717090.65",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 11400517.975539569,
            "range": "11392450.93–11408432.76",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 876019.6804097788,
            "range": "869685.88–881921.66",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12060128,
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
            "value": 105.94909599999664,
            "range": "85.29–109.66",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 85.28826299999491, 96.11183100001654, 105.94909599999664, 109.25582699998631, 109.66208899999037"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 432.7105870000087,
            "range": "409.65–496.19",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 409.64920399998664, 414.9107110000041, 432.7105870000087, 465.94309399998747, 496.19410199997947"
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
          "id": "b0a0396b05c1022d47fa06936fb7e433073b6219",
          "message": "Merge pull request #259 from sims1253/fix/arithmetic-result-modes\n\nfix(checker): infer arithmetic results by operator",
          "timestamp": "2026-09-07T03:58:33+02:00",
          "tree_id": "6a9d09f4b270e08a6e94457c0f47ee2eb797d68b",
          "url": "https://github.com/sims1253/ry/commit/b0a0396b05c1022d47fa06936fb7e433073b6219"
        },
        "date": 1788746596130,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 9073318.702005733,
            "range": "9055905.79–9095755.73",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 1469215.2221981762,
            "range": "1467700.21–1471104.22",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 8947173.442950185,
            "range": "8833500.84–9110056.77",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 4455897.5535041625,
            "range": "4426378.79–4507575.49",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 990050.3902791094,
            "range": "986503.58–993487.79",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 1712284.7443823596,
            "range": "1707492.56–1719292.45",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 587833.8783509844,
            "range": "587408.06–588325.36",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 4440146.607638354,
            "range": "4429766.80–4454287.73",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 963402.2455883082,
            "range": "962726.20–964159.81",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 35871634,
            "range": "35543531.44–36274383.96",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 8298164.226231277,
            "range": "8241691.76–8379090.26",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 3978924.0497421026,
            "range": "3969652.51–3994245.18",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 8172607.737496823,
            "range": "8081101.38–8312549.45",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 3241640.5219553052,
            "range": "3139726.76–3388833.26",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 10057833.204677735,
            "range": "9851671.88–10308579.05",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 912556.0688808721,
            "range": "908287.86–917079.47",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12060336,
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
            "value": 78.07048799999757,
            "range": "71.01–83.03",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 71.00832500000251, 76.3231810000143, 78.07048799999757, 78.73681100000977, 83.02683300001081"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 365.38230500000645,
            "range": "359.36–453.82",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 359.36482000001706, 363.79085399999167, 365.38230500000645, 375.31340899999486, 453.82411000001593"
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
          "id": "5d58a9c4526b6036bba687fffec57f60808cda3e",
          "message": "Merge pull request #262 from sims1253/fix/data-frame-arithmetic-columns\n\nfix(checker): compute scalar data-frame arithmetic columns",
          "timestamp": "2026-09-07T03:59:40+02:00",
          "tree_id": "357e80deb5b698f26fbe5920fa5d8c26eb6aafc2",
          "url": "https://github.com/sims1253/ry/commit/5d58a9c4526b6036bba687fffec57f60808cda3e"
        },
        "date": 1788746892454,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 12220500.444408137,
            "range": "12141396.23–12297288.98",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 1976678.325821252,
            "range": "1962946.77–1989732.30",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 12611667.504535196,
            "range": "12496829.36–12722370.88",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6075294.778361632,
            "range": "6003421.13–6140692.57",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1353159.5304364807,
            "range": "1344812.25–1360672.86",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2439990.596912443,
            "range": "2426571.57–2452831.25",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 840432.5023620315,
            "range": "836434.95–844230.02",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6158392.189489229,
            "range": "6115791.19–6197946.66",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1326574.7466343686,
            "range": "1315739.71–1336763.40",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 57794607.4,
            "range": "57407864.94–58200593.07",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 10537640.435487548,
            "range": "10477666.46–10606539.82",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5300734.839275926,
            "range": "5219017.12–5393332.07",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 11013855.07785572,
            "range": "10960432.05–11066488.22",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4273998.508242321,
            "range": "4246224.02–4303813.82",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 13006696.651087213,
            "range": "12903321.44–13112487.70",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 2052643.2468168717,
            "range": "2041720.30–2061744.34",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12070344,
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
            "value": 97.06498899997678,
            "range": "71.88–141.18",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 71.8847399999504, 88.58651399996597, 97.06498899997678, 103.10107999999309, 141.18402699998114"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 462.96190599998226,
            "range": "384.41–583.54",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 384.4148939999868, 412.1313739999896, 462.96190599998226, 492.1540650000097, 583.5405029999674"
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
          "id": "da043816444756d0df1d82a67ad3342d83a238ac",
          "message": "Merge pull request #264 from sims1253/fix/integer-literal-modes\n\nfix(parser): honor R integer literal storage limits",
          "timestamp": "2026-09-07T04:16:34+02:00",
          "tree_id": "d87b76463a232d2fbef1d3bd958d0e760058a910",
          "url": "https://github.com/sims1253/ry/commit/da043816444756d0df1d82a67ad3342d83a238ac"
        },
        "date": 1788747657251,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 14089511.611063331,
            "range": "13927339.89–14259324.21",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2239145.5032159477,
            "range": "2235454.94–2244108.21",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14223995.3415556,
            "range": "14111774.69–14383012.68",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7333164.690728387,
            "range": "7282388.83–7402499.00",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1513864.916208904,
            "range": "1511278.76–1516899.67",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 3024225.029120958,
            "range": "3000111.70–3061749.99",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 1019202.362164213,
            "range": "1011777.05–1032139.87",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7314348.84242395,
            "range": "7295678.96–7340739.12",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1500112.974069593,
            "range": "1498339.72–1502251.16",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 65638047.966666676,
            "range": "65453773.47–65830708.67",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12820321.800624741,
            "range": "12788185.29–12849006.76",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5849874.505761833,
            "range": "5780413.73–5934367.82",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12851007.443525277,
            "range": "12808952.31–12895701.90",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4887425.567832009,
            "range": "4874169.53–4903503.76",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 16013215.168635413,
            "range": "15857070.71–16238520.39",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1932860.1280376525,
            "range": "1924015.34–1942397.82",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12070712,
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
            "value": 153.80647499999031,
            "range": "109.93–165.57",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 109.92576800001552, 138.18461900000693, 153.80647499999031, 156.59173500002362, 165.5679179999861"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 485.91083499998786,
            "range": "467.26–558.01",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 467.2579109999933, 468.4073549999739, 485.91083499998786, 542.4542590000201, 558.0065610000165"
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
          "id": "922331c04f339118854da1cda0982885b9957661",
          "message": "Merge pull request #261 from sims1253/fix/class-assignment-provenance\n\nFix class assignment provenance and storage uncertainty",
          "timestamp": "2026-09-07T04:19:29+02:00",
          "tree_id": "f9feb1ab172f652ebda6975971e19cb10c4b9089",
          "url": "https://github.com/sims1253/ry/commit/922331c04f339118854da1cda0982885b9957661"
        },
        "date": 1788747930439,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 22809688.026509102,
            "range": "22266093.27–23353990.57",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2317676.477848004,
            "range": "2311736.56–2324351.48",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 15098005.339713564,
            "range": "14948215.46–15287387.13",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7025244.807265565,
            "range": "7001116.94–7055640.26",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1478973.3921376814,
            "range": "1476052.18–1482234.42",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 3065288.5652502133,
            "range": "3046731.43–3097741.13",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 1015923.8136638269,
            "range": "1012919.71–1020484.12",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7001034.583696413,
            "range": "6991650.12–7012242.67",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1484301.2560256952,
            "range": "1475858.06–1497145.66",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 73174485.2,
            "range": "73020516.79–73349726.19",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 13227145.189507427,
            "range": "13204519.28–13252215.85",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5757915.966165794,
            "range": "5739226.89–5785252.21",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 13608554.849293154,
            "range": "13544844.76–13664956.91",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5222218.368946302,
            "range": "5179358.21–5281706.76",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 16539197.771702971,
            "range": "16379759.53–16768393.56",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1927392.6681707683,
            "range": "1924194.91–1931060.49",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12079008,
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
            "value": 125.87296499998774,
            "range": "124.30–216.16",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 124.3019600000116, 124.9563349999953, 125.87296499998774, 155.4248479999951, 216.15563100000145"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 506.9663130000117,
            "range": "481.40–724.38",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 481.4005730000208, 500.16617899999255, 506.9663130000117, 517.5097999999998, 724.3764760000049"
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
          "id": "2744096c9bbb87ad9f0f5caee4d43ac60fcbdf10",
          "message": "Merge pull request #270 from sims1253/docs/cli-command-help\n\nKeep CLI command summaries concise",
          "timestamp": "2026-09-07T04:48:41+02:00",
          "tree_id": "0c7c5e75703a4eae25f4256507e3effe9dbb08c3",
          "url": "https://github.com/sims1253/ry/commit/2744096c9bbb87ad9f0f5caee4d43ac60fcbdf10"
        },
        "date": 1788749596728,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 15033209.896212041,
            "range": "14724334.36–15410369.88",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2236621.811714032,
            "range": "2231806.20–2242174.19",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14413367.921415174,
            "range": "14343598.77–14481560.70",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7441806.43991719,
            "range": "7413768.25–7469610.03",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1525729.2386941568,
            "range": "1509688.99–1546958.50",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 3160152.981176858,
            "range": "3144970.33–3178214.60",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 1034708.2152576183,
            "range": "1032056.34–1037526.27",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7358302.021914899,
            "range": "7334051.88–7384157.43",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1511332.0409102298,
            "range": "1503634.65–1522330.17",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 67637716.25000001,
            "range": "67381018.67–67910881.40",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12799447.277656153,
            "range": "12731553.40–12871235.50",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5752541.077083423,
            "range": "5742478.05–5763274.02",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12935684.01415304,
            "range": "12900857.48–12972706.69",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5010834.5397201665,
            "range": "4985303.50–5047019.32",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 16464996.7912088,
            "range": "16187801.61–16807602.75",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1944885.662846235,
            "range": "1936559.91–1953544.62",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12079168,
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
            "value": 141.4835740000126,
            "range": "128.70–153.60",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 128.6950229999493, 133.18683800002327, 141.4835740000126, 151.67907499999274, 153.60119700001087"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 518.750961999991,
            "range": "449.70–530.94",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 449.7032439999748, 484.04756899998756, 518.750961999991, 522.9198979999637, 530.9447710000095"
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
          "id": "4c1f44836fe225ce1fb4099787501dc0c68e7984",
          "message": "Merge pull request #269 from sims1253/docs/persistent-map-screening\n\nRecord rejected persistent Scope map screening",
          "timestamp": "2026-09-07T04:53:34+02:00",
          "tree_id": "ef9afb9c707ad29c79a79ca513cebd3d03a81f76",
          "url": "https://github.com/sims1253/ry/commit/4c1f44836fe225ce1fb4099787501dc0c68e7984"
        },
        "date": 1788749852668,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 10587569.315620983,
            "range": "10566810.24–10611277.93",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 1720126.3902958217,
            "range": "1717788.35–1722824.61",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 10285015.288492126,
            "range": "10189875.51–10418223.87",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 5424792.160786887,
            "range": "5419079.63–5430695.75",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1128379.0171085834,
            "range": "1126947.48–1130240.45",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2341422.0860556844,
            "range": "2339231.94–2343896.97",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 779833.6095499328,
            "range": "775071.93–785549.10",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 5437619.289160605,
            "range": "5431563.99–5443267.97",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1124351.8495203792,
            "range": "1123145.32–1125814.66",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 51644724.23333334,
            "range": "51510021.75–51789043.16",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 9282591.597317152,
            "range": "9264323.97–9303486.36",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 4425399.951405946,
            "range": "4409068.35–4447701.54",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 9285036.327525895,
            "range": "9269693.60–9300760.15",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 3574824.4283896224,
            "range": "3563748.92–3588796.42",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 11558613.415370736,
            "range": "11540268.93–11583017.04",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1503895.6038829912,
            "range": "1499512.36–1508206.27",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12079168,
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
            "value": 91.68421300000045,
            "range": "88.84–98.80",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 88.84157499996945, 89.18656699999701, 91.68421300000045, 98.41821999999229, 98.80315599997994"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 395.5417080000043,
            "range": "386.88–431.26",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 386.87933999998495, 392.59880299994256, 395.5417080000043, 416.50798600004055, 431.2570060000289"
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
          "id": "9df07adb964070fce1184a529312d25d6120afbe",
          "message": "Merge pull request #267 from sims1253/fix/custom-operator-lookup\n\nfix: resolve custom binary Ops before their operands",
          "timestamp": "2026-09-07T05:09:42+02:00",
          "tree_id": "1c212b19aaa1e66850b990912383a0f8c2fa6ed3",
          "url": "https://github.com/sims1253/ry/commit/9df07adb964070fce1184a529312d25d6120afbe"
        },
        "date": 1788750875747,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 11176480.144417096,
            "range": "11153726.15–11203046.60",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 1849204.1289047948,
            "range": "1819550.86–1894463.13",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 11265909.705785777,
            "range": "11195140.01–11381362.53",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 5658891.132303149,
            "range": "5649464.05–5670524.01",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1230447.1499841074,
            "range": "1228924.20–1232905.88",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2270902.1757863294,
            "range": "2263747.14–2280496.65",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 759203.0577465168,
            "range": "757672.34–760912.15",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 5669483.6536004115,
            "range": "5660802.14–5683590.31",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1232659.773196473,
            "range": "1230450.78–1235458.92",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 47226830.7625,
            "range": "46822418.76–47791903.91",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 10310691.650469368,
            "range": "10135541.17–10532549.24",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 4636334.887606502,
            "range": "4610645.15–4669493.56",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 10215433.236987505,
            "range": "10205053.09–10229562.80",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4008884.2642360763,
            "range": "3994451.50–4034479.39",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 12300696.370854054,
            "range": "12283698.32–12316515.14",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1952202.4853926692,
            "range": "1934908.94–1971822.97",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12252448,
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
            "value": 99.87998299999163,
            "range": "79.52–113.89",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 79.51874899998074, 90.06756900000619, 99.87998299999163, 109.79116900003282, 113.89335799997207"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 420.0462890000199,
            "range": "369.51–689.11",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 369.5073620000039, 406.72641000000294, 420.0462890000199, 525.0874460000196, 689.1102899999823"
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
          "id": "4eb045b4de7ac0553c544dbc44f18578cfbe6da3",
          "message": "Merge pull request #266 from sims1253/fix/factor-new-provenance\n\nProve factor and methods new constructor calls",
          "timestamp": "2026-09-07T05:13:48+02:00",
          "tree_id": "9bbbf23fbec6899213c4d27a45de4e78b946f9aa",
          "url": "https://github.com/sims1253/ry/commit/4eb045b4de7ac0553c544dbc44f18578cfbe6da3"
        },
        "date": 1788751156380,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 12999168.580116639,
            "range": "12852895.58–13166932.70",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2114009.3574530496,
            "range": "2109089.62–2119921.10",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 12732540.139942545,
            "range": "12672334.63–12795021.36",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6303686.76062067,
            "range": "6284685.32–6326531.12",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1393714.058171552,
            "range": "1389664.28–1398214.97",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2739480.776844679,
            "range": "2735978.44–2744496.97",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 933492.6865578521,
            "range": "932665.57–934393.23",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6310669.678606953,
            "range": "6295163.28–6327883.81",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1393166.053933842,
            "range": "1389341.89–1397045.03",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 69552131.58333333,
            "range": "69361852.42–69749893.49",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 11389210.660392363,
            "range": "11354096.64–11425699.53",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5435148.127446441,
            "range": "5420703.54–5452060.84",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 11657937.79461952,
            "range": "11579145.84–11777258.36",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4422281.372186402,
            "range": "4405299.15–4444657.54",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 14047623.863807699,
            "range": "14002375.84–14095262.07",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 2184946.8623864986,
            "range": "2171963.94–2205789.88",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12261280,
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
            "value": 124.80810299998848,
            "range": "112.54–133.60",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 112.54339400003664, 120.19538900000043, 124.80810299998848, 125.94956399995135, 133.59727499994915"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 466.89068899996346,
            "range": "443.21–484.75",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 443.2138630000409, 457.9742639999604, 466.89068899996346, 469.0134450000478, 484.7451960000326"
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
          "id": "1ca4235b21186c5af49d87b9d6bc68c08b9861b0",
          "message": "Merge pull request #273 from sims1253/perf/sparse-promise-capture-index\n\nAvoid indexing functions without promise captures",
          "timestamp": "2026-09-07T05:38:33+02:00",
          "tree_id": "79541cd82bc9560382ce97721c0944c13f5db094",
          "url": "https://github.com/sims1253/ry/commit/1ca4235b21186c5af49d87b9d6bc68c08b9861b0"
        },
        "date": 1788752603397,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 21767016.81286102,
            "range": "21296818.93–22175372.03",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2202328.881163915,
            "range": "2187889.36–2216291.58",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14466480.04222239,
            "range": "14298022.42–14623503.18",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6674622.151691144,
            "range": "6638771.09–6706016.94",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1399155.5390815123,
            "range": "1386183.15–1421770.15",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2898353.6950592115,
            "range": "2872987.97–2922401.21",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 938695.0393383509,
            "range": "921418.18–963305.41",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6657608.48522787,
            "range": "6611241.52–6700550.91",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1401264.4281214091,
            "range": "1398045.18–1404888.56",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 75991439.1,
            "range": "75311840.86–76623518.04",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 13231529.857647244,
            "range": "13078003.07–13379068.58",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5689793.083790144,
            "range": "5663156.07–5714945.51",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 13368687.989701506,
            "range": "13227355.16–13502142.13",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5574349.852783481,
            "range": "5440128.72–5694474.38",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 16311408.55939799,
            "range": "16091299.82–16608573.50",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 2288225.7660444314,
            "range": "2279314.56–2296726.28",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12261888,
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
            "value": 133.4842930000159,
            "range": "123.91–160.57",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 123.91396099998383, 124.46630099997856, 133.4842930000159, 143.39225400000578, 160.5693850000389"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 501.7580660000094,
            "range": "467.71–544.32",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 467.71132800000487, 487.4486790000228, 501.7580660000094, 505.1412939999718, 544.3198499999708"
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
          "id": "6c64c1f58e8be42c912c3f8690b3a66e7c179aa1",
          "message": "Merge pull request #271 from sims1253/fix/missing-actuals\n\nfix(parser): preserve omitted call arguments",
          "timestamp": "2026-09-07T05:41:31+02:00",
          "tree_id": "14f3886c231554bbad34fb04f079207162698c85",
          "url": "https://github.com/sims1253/ry/commit/6c64c1f58e8be42c912c3f8690b3a66e7c179aa1"
        },
        "date": 1788752832351,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 11524315.513564663,
            "range": "11309982.96–11796745.98",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 1727600.706573875,
            "range": "1725274.64–1731042.93",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 10508630.151315462,
            "range": "10419318.57–10619332.80",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 5402244.988462896,
            "range": "5392598.67–5413161.76",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1150872.1522705075,
            "range": "1141537.62–1162906.34",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2365356.9399024746,
            "range": "2360440.05–2371182.74",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 776063.0720276732,
            "range": "774660.34–777758.40",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 5439344.948941416,
            "range": "5412247.58–5483962.64",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1135843.790976157,
            "range": "1134875.98–1136937.67",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 64906692.13333334,
            "range": "64339430.00–65433644.13",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 9526102.948034411,
            "range": "9494866.11–9562234.50",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 4453712.594768918,
            "range": "4436872.50–4476326.52",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 9558080.314615743,
            "range": "9472046.45–9696929.03",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 3694883.298992531,
            "range": "3689154.93–3698842.11",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 11937272.887081042,
            "range": "11857261.86–12043912.94",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1478470.6247868426,
            "range": "1471575.86–1485528.87",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12262256,
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
            "value": 96.91274900001008,
            "range": "89.56–114.04",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 89.56441900000209, 95.8604200000118, 96.91274900001008, 97.6668349999818, 114.04311400000006"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 426.0041440000059,
            "range": "415.27–479.44",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 415.26892099998076, 417.467284000013, 426.0041440000059, 431.13526999999885, 479.43960000001243"
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
          "id": "d9902bc2a0989a24d3bffff7602a353912b0a6b7",
          "message": "Organize Unreleased notes and correct upgrade guidance (#272)\n\n* Organize Unreleased notes and correct upgrade guidance\n\n* Describe dump-types scope coverage accurately",
          "timestamp": "2026-09-07T05:52:24+02:00",
          "tree_id": "7d6a8649d3732748c56a809bbf33bbf4fe534073",
          "url": "https://github.com/sims1253/ry/commit/d9902bc2a0989a24d3bffff7602a353912b0a6b7"
        },
        "date": 1788753405425,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 15214823.948328506,
            "range": "14877510.01–15578330.87",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2286614.484602779,
            "range": "2283180.48–2290452.22",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14989751.95385485,
            "range": "14868076.26–15127046.86",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6936841.762612423,
            "range": "6919061.16–6958270.73",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1504548.7935894113,
            "range": "1489451.66–1526703.07",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 3150207.991713483,
            "range": "3101321.48–3213886.23",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 1022823.6519988838,
            "range": "1020580.34–1025332.39",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6977108.781584417,
            "range": "6958980.90–6997560.70",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1479368.3159937852,
            "range": "1476911.65–1482069.33",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 79453673.375,
            "range": "78409064.67–80450261.89",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 13267035.691495126,
            "range": "13241370.55–13303331.19",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5733001.028065871,
            "range": "5725495.23–5741527.59",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 13234823.319603797,
            "range": "13211340.27–13258176.88",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5165996.146771669,
            "range": "5134884.56–5199212.37",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 16703625.578897577,
            "range": "16517705.08–16965231.00",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1930979.0664952793,
            "range": "1921412.62–1941549.42",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12262256,
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
            "value": 128.35554600000614,
            "range": "111.40–158.21",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 111.40327499998966, 125.3678099999961, 128.35554600000614, 141.393167000002, 158.20819299999857"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 480.42140299998573,
            "range": "452.84–511.86",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 452.83802500000456, 465.4740340000135, 480.42140299998573, 501.6709549999796, 511.8609080000024"
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
          "id": "598228c75d3898e3c946e7a5f584da48ecca991f",
          "message": "Merge pull request #277 from sims1253/fix/deferred-call-provenance\n\nRespect custom hasArg and on.exit functions",
          "timestamp": "2026-09-07T10:43:23+02:00",
          "tree_id": "4a118025f4ac3471fd27c2828aff29bfebc6a3c8",
          "url": "https://github.com/sims1253/ry/commit/598228c75d3898e3c946e7a5f584da48ecca991f"
        },
        "date": 1788771055058,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 13488055.13194944,
            "range": "13374799.30–13626031.58",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2216300.8465672084,
            "range": "2207291.94–2229467.81",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14391441.519132074,
            "range": "14294490.75–14525068.92",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7428636.415911669,
            "range": "7408203.53–7449201.11",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1515831.253567368,
            "range": "1493781.52–1551917.39",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 3045204.4549577725,
            "range": "3030421.51–3061540.29",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 1005981.9745343194,
            "range": "1003685.47–1009316.52",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7408723.8179899575,
            "range": "7390530.22–7427105.99",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1485761.9548764045,
            "range": "1483391.95–1488557.13",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 71328802.28333335,
            "range": "70674593.19–72025698.52",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12945595.044439279,
            "range": "12919842.04–12972560.27",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5705360.630873486,
            "range": "5693477.55–5720369.37",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12864041.911810143,
            "range": "12819519.46–12910779.86",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5012868.231617838,
            "range": "4978657.64–5069707.28",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 16152254.916590026,
            "range": "15967914.81–16384988.19",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1939355.301444374,
            "range": "1917018.72–1972362.99",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12263488,
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
            "value": 143.070223000017,
            "range": "121.91–179.74",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 121.9051750000217, 129.6204349999898, 143.070223000017, 147.74269400001504, 179.73797399998875"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 510.1284299999825,
            "range": "455.23–528.13",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 455.22879799996736, 491.9137810000102, 510.1284299999825, 525.8224900000496, 528.1337489999714"
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
          "id": "d1499fa03c708cffdcb397fb802d33c6c0789c6a",
          "message": "Merge pull request #279 from sims1253/fix/printf-provenance\n\nCheck printf arity only for proven base formatters",
          "timestamp": "2026-09-07T10:47:34+02:00",
          "tree_id": "f3ee6fa0b9fb1975dbf9527a42ed3a4f9085ea86",
          "url": "https://github.com/sims1253/ry/commit/d1499fa03c708cffdcb397fb802d33c6c0789c6a"
        },
        "date": 1788771334428,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 14022446.173907474,
            "range": "13991132.60–14059148.70",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2276517.567097085,
            "range": "2273329.11–2280778.21",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14599323.4667208,
            "range": "14522216.29–14697880.06",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6980898.34325383,
            "range": "6972717.10–6990121.33",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1474486.3123047045,
            "range": "1470239.81–1481373.72",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 3039265.4489446376,
            "range": "3037307.56–3041340.71",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 1013547.7940767299,
            "range": "1005930.95–1027743.35",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7019808.954978901,
            "range": "7012931.25–7027008.17",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1467353.2970529192,
            "range": "1464139.26–1470833.45",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 75245394.85,
            "range": "74460276.60–76062215.07",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12980959.412557133,
            "range": "12966622.25–12995577.85",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5651732.062092829,
            "range": "5644952.49–5658762.92",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 13018208.930596355,
            "range": "13003770.50–13031885.34",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5043568.556367388,
            "range": "5027724.38–5060566.88",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 16507986.57265622,
            "range": "16335064.08–16738201.51",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1927484.2187135827,
            "range": "1925103.49–1930367.46",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12263976,
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
            "value": 127.89386099996045,
            "range": "112.89–133.14",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 112.89333399996394, 120.20504799997434, 127.89386099996045, 129.80379400000675, 133.14206499996362"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 493.29124599997886,
            "range": "468.38–515.13",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 468.3849399999599, 479.2375609999872, 493.29124599997886, 501.16624699998647, 515.132826999994"
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
          "id": "03456211250945922f681294246207ee1f23b661",
          "message": "Merge pull request #276 from sims1253/docs/audit-snapshot-counts\n\ndocs: reconcile historical audit counts across evidence pages",
          "timestamp": "2026-09-07T10:58:16+02:00",
          "tree_id": "a5228e62968fb8b473143a38658330669fb86fcc",
          "url": "https://github.com/sims1253/ry/commit/03456211250945922f681294246207ee1f23b661"
        },
        "date": 1788771747846,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 10287166.208988376,
            "range": "9834777.28–10747341.45",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 1356431.1111472135,
            "range": "1343528.78–1368387.32",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 7867358.802728725,
            "range": "7826167.00–7911021.63",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 4484995.885651324,
            "range": "4458820.74–4512464.73",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 920416.9192558953,
            "range": "910098.32–930299.93",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 1797543.2270761419,
            "range": "1747681.74–1883476.55",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 585809.704903371,
            "range": "582560.36–588797.29",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 4474182.040915437,
            "range": "4427578.90–4535716.17",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 983139.0500726585,
            "range": "946090.40–1032408.78",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 52531095.48333332,
            "range": "51693638.51–53285873.31",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 7677485.810329805,
            "range": "7612115.76–7738986.44",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 3363886.2795484215,
            "range": "3319974.77–3416712.80",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 7655110.421659162,
            "range": "7568584.38–7742249.57",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 2885884.778983699,
            "range": "2833142.97–2949715.64",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 9152153.1499384,
            "range": "9042869.11–9274252.58",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1496694.8489754891,
            "range": "1481582.56–1517191.96",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12263976,
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
            "value": 78.34653300000355,
            "range": "53.91–101.24",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 53.91317400004482, 67.6974250000203, 78.34653300000355, 93.20588300004601, 101.24083299998892"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 413.66074699995806,
            "range": "350.76–580.17",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 350.76463499997044, 409.4243710000301, 413.66074699995806, 426.02069899998605, 580.1747840000317"
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
          "id": "8d6cd395e74d1d403253a76fc3675d29006d42f9",
          "message": "Merge pull request #274 from sims1253/fix/r-string-escape-decoding\n\nDecode R string escapes without corrupting byte sequences",
          "timestamp": "2026-09-07T11:07:16+02:00",
          "tree_id": "2896b0324ea8840fc3abab383b305a4dba66080f",
          "url": "https://github.com/sims1253/ry/commit/8d6cd395e74d1d403253a76fc3675d29006d42f9"
        },
        "date": 1788772348713,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 16062913.465456482,
            "range": "15696323.94–16425725.57",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2321420.7355908686,
            "range": "2197070.40–2555248.27",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14247099.141236257,
            "range": "14209603.31–14287569.18",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7401856.8266879795,
            "range": "7380348.68–7428067.60",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1508375.8739543466,
            "range": "1490552.23–1533574.88",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 3046422.0664224736,
            "range": "3034083.93–3059664.32",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 1000782.658707782,
            "range": "998566.21–1003878.32",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7398637.83099134,
            "range": "7369019.22–7427862.02",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1484953.6042892798,
            "range": "1483965.97–1485938.49",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 70359436.68333332,
            "range": "70075674.35–70703383.30",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12813958.638636595,
            "range": "12784163.30–12844289.71",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5798031.01906207,
            "range": "5786105.34–5809584.14",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12843631.774070565,
            "range": "12801547.04–12887662.93",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4970321.982581521,
            "range": "4955015.68–4988175.92",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 16410001.368008917,
            "range": "16232606.87–16668909.54",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1911018.4365545094,
            "range": "1908681.52–1913546.59",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12263240,
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
            "value": 132.96861199999694,
            "range": "114.19–160.94",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 114.18586699996376, 131.09970099997008, 132.96861199999694, 146.24033000000054, 160.94019299995853"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 481.09492599999066,
            "range": "458.48–505.10",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 458.47784700000193, 462.00773399998434, 481.09492599999066, 490.7971949999919, 505.10128399997484"
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
          "id": "a22502183b2b917d1aa0a64276325458aa92033e",
          "message": "Merge pull request #281 from sims1253/fix/r-surrogate-pairs\n\nfix(parser): decode valid Unicode surrogate pairs",
          "timestamp": "2026-09-07T11:07:42+02:00",
          "tree_id": "a998e75af9df611932b24cfd939d946b099c6f43",
          "url": "https://github.com/sims1253/ry/commit/a22502183b2b917d1aa0a64276325458aa92033e"
        },
        "date": 1788772668545,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 15442951.91022693,
            "range": "15306606.28–15578819.77",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2279851.779749075,
            "range": "2277264.13–2283181.54",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14596694.753274005,
            "range": "14478667.05–14767728.98",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6903619.865865511,
            "range": "6894923.11–6914014.30",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1474754.2772894832,
            "range": "1467957.09–1487081.64",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 3026437.204524293,
            "range": "3021851.81–3032057.99",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 1014551.0667113323,
            "range": "1013354.55–1015939.49",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6911567.501033577,
            "range": "6904230.54–6919954.43",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1463092.2837893586,
            "range": "1460945.74–1465566.92",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 72253555.9,
            "range": "71804244.44–72724416.68",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 13027088.88008279,
            "range": "12947873.06–13132715.29",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5714858.189987568,
            "range": "5680428.44–5768554.45",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 13015637.976777602,
            "range": "12999657.76–13034386.41",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5162609.979616636,
            "range": "5055259.91–5304487.08",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 16516971.8990112,
            "range": "16321057.65–16777980.62",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1931043.126929647,
            "range": "1924581.21–1937833.53",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12263520,
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
            "value": 131.56864700000733,
            "range": "123.11–155.24",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 123.1081070000073, 126.79880400002003, 131.56864700000733, 147.6924439999857, 155.23675899999216"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 496.20216599997366,
            "range": "456.33–512.28",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 456.3331710000057, 486.6930500000017, 496.20216599997366, 505.8260869999649, 512.2790089999908"
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
          "id": "5e52d8ebce02a44b0b0c8c29f3ab27ef31fcb23a",
          "message": "Merge pull request #283 from sims1253/fix/parser-depth-limit\n\nfix(parser): reject excessive syntax nesting before AST conversion",
          "timestamp": "2026-09-07T11:22:22+02:00",
          "tree_id": "8aec2dd393986b2427ec5f5fa9bcbe3b5c0f6c0c",
          "url": "https://github.com/sims1253/ry/commit/5e52d8ebce02a44b0b0c8c29f3ab27ef31fcb23a"
        },
        "date": 1788773199667,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 14984613.053713018,
            "range": "14755289.85–15232646.65",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2199914.211189486,
            "range": "2197798.67–2202027.54",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14657972.35111908,
            "range": "14455720.55–14947081.55",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7297680.879968369,
            "range": "7282305.33–7316677.49",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1502702.6580119475,
            "range": "1500054.35–1505863.83",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 3053436.5835957914,
            "range": "3043753.31–3064490.85",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 1022533.3724955712,
            "range": "1020468.52–1025164.78",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7319713.767134641,
            "range": "7302198.18–7343119.13",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1499382.108651542,
            "range": "1497152.88–1502272.38",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 75369385.675,
            "range": "74927206.88–75860919.64",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12989290.531325327,
            "range": "12940864.58–13046635.53",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5926422.769115045,
            "range": "5887393.39–5975730.54",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 13050819.081446758,
            "range": "12967942.09–13185423.50",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4963781.241214327,
            "range": "4954757.76–4974790.80",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 16275216.671408543,
            "range": "16100850.87–16507753.18",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1952780.973616851,
            "range": "1948877.00–1957204.90",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12263992,
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
            "value": 130.57529700000305,
            "range": "123.03–144.31",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 123.03346200002125, 123.33428200002527, 130.57529700000305, 142.80701399996178, 144.31429199996637"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 470.0550790000125,
            "range": "456.85–500.82",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 456.8532290000003, 457.8167970000068, 470.0550790000125, 490.35210600000573, 500.81766199995764"
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
          "id": "88b465e3a9ad819f609ebfe4dce17f9161f1cab3",
          "message": "Merge pull request #275 from sims1253/fix/initial-workspace-diagnostics\n\nWait for workspace context before initial editor diagnostics",
          "timestamp": "2026-09-07T11:23:28+02:00",
          "tree_id": "1ea8cbe7f6675e520844cf059841140dec3dede4",
          "url": "https://github.com/sims1253/ry/commit/88b465e3a9ad819f609ebfe4dce17f9161f1cab3"
        },
        "date": 1788773471995,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 16062977.430386659,
            "range": "15776737.15–16430589.52",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2257427.0245845024,
            "range": "2251570.93–2266080.23",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14469616.14045994,
            "range": "14405965.58–14572954.25",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6893072.4301995095,
            "range": "6884513.92–6902220.59",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1455570.7766794846,
            "range": "1453772.10–1457449.17",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 3001630.1185753504,
            "range": "2997364.36–3006934.07",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 1002325.1151302967,
            "range": "1000512.53–1005042.20",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6932128.882496062,
            "range": "6913991.93–6955141.49",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1465685.128768231,
            "range": "1450114.39–1491379.02",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 78822497.625,
            "range": "78424927.97–79221089.09",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12975976.529172365,
            "range": "12958665.49–12991665.16",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5780246.127231046,
            "range": "5754998.72–5808336.53",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 13123361.499133533,
            "range": "13064032.64–13195210.18",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5026678.984536077,
            "range": "5007149.60–5054827.25",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15495209.361778116,
            "range": "15387143.87–15631070.38",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1939727.6097918716,
            "range": "1932223.52–1947338.15",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12277112,
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
            "value": 131.39055399998324,
            "range": "127.80–168.06",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 127.79847700003302, 127.86562799999956, 131.39055399998324, 135.79195899999468, 168.0578220000025"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 482.1785989999771,
            "range": "466.12–497.74",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 466.1206280000042, 466.2324410000001, 482.1785989999771, 486.4632800000254, 497.74023599998327"
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
          "id": "6fdcdc69e4f1404022e3be59988f6cdab5323bde",
          "message": "Merge pull request #280 from sims1253/fix/capture-formal-matching\n\nfix(checker): match captured actuals to helper formals",
          "timestamp": "2026-09-07T11:27:19+02:00",
          "tree_id": "25a2b66f28bdfb2ee24c82d1a963915591e75bd4",
          "url": "https://github.com/sims1253/ry/commit/6fdcdc69e4f1404022e3be59988f6cdab5323bde"
        },
        "date": 1788773736609,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 14072421.658825269,
            "range": "14050407.99–14094944.04",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2280800.8446480143,
            "range": "2278524.06–2283831.21",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14550855.42425389,
            "range": "14409981.93–14722949.26",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6954578.196354707,
            "range": "6944988.58–6965006.86",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1447213.0587457048,
            "range": "1445261.66–1449397.44",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 3024654.9249838544,
            "range": "3019792.01–3029096.69",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 955869.9647852449,
            "range": "949195.28–966652.59",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6964958.952173762,
            "range": "6954276.40–6980693.93",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1446648.1957526274,
            "range": "1442848.26–1451590.20",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 71129672.28333333,
            "range": "70544374.64–71856100.03",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12928938.71456216,
            "range": "12910566.96–12947965.83",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5726454.500811227,
            "range": "5716236.83–5735900.06",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12964403.384183304,
            "range": "12939272.83–12991711.98",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5047961.531919299,
            "range": "5033533.73–5065232.17",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15514846.38761608,
            "range": "15410605.25–15665040.84",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1927446.2745838906,
            "range": "1923293.96–1932254.46",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12285632,
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
            "value": 132.24559499998577,
            "range": "109.99–139.56",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 109.98796800000127, 128.9485069999937, 132.24559499998577, 137.27909599998384, 139.5646429999906"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 469.6025920000102,
            "range": "452.10–484.37",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 452.1041710000136, 458.5139519999793, 469.6025920000102, 473.0989899999695, 484.37061400001403"
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
          "id": "d5414ffe2be45d2f3ead7a804ea7a872c80f6346",
          "message": "Merge pull request #282 from sims1253/fix/plain-vector-ops\n\nProve plain-vector fallback for explicit Ops chooser refusals",
          "timestamp": "2026-09-07T11:40:15+02:00",
          "tree_id": "dc782208f4cb8e59295c57513fa86dbba62c651b",
          "url": "https://github.com/sims1253/ry/commit/d5414ffe2be45d2f3ead7a804ea7a872c80f6346"
        },
        "date": 1788774309228,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 23336743.248346828,
            "range": "21441285.19–25179420.38",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2524107.336984021,
            "range": "2515120.07–2532976.65",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 16248845.231083805,
            "range": "16136653.12–16397591.86",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 8137790.16606701,
            "range": "8100898.95–8177955.04",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1697709.9044256578,
            "range": "1661106.95–1757126.17",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 3408884.744328881,
            "range": "3396357.25–3419842.92",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 1047525.1804734872,
            "range": "1043598.13–1051995.23",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 8042976.088562126,
            "range": "8014621.18–8071814.21",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1646373.570738708,
            "range": "1641168.24–1651490.36",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 104501795.45,
            "range": "102628787.72–106285959.27",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 14915800.045457786,
            "range": "14800735.19–15045134.32",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 6302354.41221145,
            "range": "6270654.69–6333351.47",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 14500993.35050633,
            "range": "14397385.29–14621830.68",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5477517.968015851,
            "range": "5449370.95–5506260.09",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 16943673.498885654,
            "range": "16751729.58–17176713.09",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 2263220.291305912,
            "range": "2244013.45–2282655.88",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12295576,
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
            "value": 189.17630599997938,
            "range": "148.98–196.65",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 148.97782400000142, 168.74250200000824, 189.17630599997938, 195.62156299996423, 196.65212400001474"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 567.059981999977,
            "range": "526.84–629.37",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 526.8386360000004, 535.8178469999693, 567.059981999977, 607.9385849999962, 629.365259000042"
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
          "id": "c27019f40defd8baa5e1af2bfb701a9cb9a3fde6",
          "message": "Merge pull request #284 from sims1253/perf/scope-130\n\nperf(infer): reduce expression-if scope allocation",
          "timestamp": "2026-09-07T12:24:05+02:00",
          "tree_id": "7c3222d0d479134723448f829b3b1d7efbb19dd8",
          "url": "https://github.com/sims1253/ry/commit/c27019f40defd8baa5e1af2bfb701a9cb9a3fde6"
        },
        "date": 1788776936999,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 14159485.391642366,
            "range": "14087652.92–14248239.45",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2315375.823190993,
            "range": "2312299.12–2319093.61",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5998720.625524005,
            "range": "5988272.28–6010908.70",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 836322.2030149853,
            "range": "835185.91–837825.23",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 9892204.603306174,
            "range": "9878940.30–9908664.21",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1345485.0010846127,
            "range": "1343722.61–1347521.87",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14588224.712132204,
            "range": "14563057.99–14618112.32",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6995199.595876919,
            "range": "6985487.02–7006844.17",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1470239.9512221138,
            "range": "1460380.69–1486565.80",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 3060774.708691421,
            "range": "3055987.15–3066546.03",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 969914.0470107121,
            "range": "967528.33–973204.92",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7019820.5155992955,
            "range": "7012843.94–7028029.51",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1459993.6154458718,
            "range": "1457160.34–1464044.04",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 76958918.925,
            "range": "75262311.25–79052412.25",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12875855.054285929,
            "range": "12853348.16–12901268.53",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5705052.683643685,
            "range": "5690003.25–5720458.38",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12880602.66781031,
            "range": "12860480.96–12903374.73",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4994868.553341928,
            "range": "4990396.15–5000129.72",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15380183.109846894,
            "range": "15258189.17–15542134.97",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1933950.0421515338,
            "range": "1926905.02–1945056.75",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12296280,
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
            "value": 131.26656199997524,
            "range": "127.65–165.08",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 127.64797299995553, 128.15582400001585, 131.26656199997524, 134.32517899997765, 165.08498300000792"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 464.95627899997635,
            "range": "444.14–492.67",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 444.1350220000022, 462.08723800000735, 464.95627899997635, 473.65762399998493, 492.669755000039"
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
          "id": "d2c3a4c94e3b95ae0308f0c35df5edc18fdce574",
          "message": "Merge pull request #288 from sims1253/fix/callable-audit\n\nfix(infer): discard prior types after exhaustive branch assignment",
          "timestamp": "2026-09-07T12:30:43+02:00",
          "tree_id": "08b98a911dac271d1941514d2ad84d01a432eede",
          "url": "https://github.com/sims1253/ry/commit/d2c3a4c94e3b95ae0308f0c35df5edc18fdce574"
        },
        "date": 1788777362175,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 16994492.064212967,
            "range": "16631609.51–17377888.22",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2256395.5124495225,
            "range": "2253463.64–2259877.04",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5954118.704398143,
            "range": "5898836.09–6026385.30",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 830103.1339763685,
            "range": "822643.30–842065.45",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 10025950.538066218,
            "range": "9957139.76–10099458.43",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1343320.1636731513,
            "range": "1336693.86–1354275.35",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 15133073.282512993,
            "range": "14945293.71–15427969.53",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7501628.675163363,
            "range": "7483261.26–7521149.52",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1489418.542842109,
            "range": "1486528.90–1493360.44",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 3101088.8947397294,
            "range": "3092944.65–3109404.87",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 958700.4299943692,
            "range": "956863.47–960620.38",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7557273.018949596,
            "range": "7506531.79–7631118.34",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1484839.9741600158,
            "range": "1481895.43–1488228.15",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 74656718.425,
            "range": "74450492.14–74850447.29",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12976966.025255872,
            "range": "12948166.85–13006593.87",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5736298.4941263795,
            "range": "5719422.60–5751169.97",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 13104632.408667246,
            "range": "13021748.02–13190291.40",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4997369.254298886,
            "range": "4957896.09–5049087.16",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15499968.131142098,
            "range": "15381084.47–15644698.03",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1990255.429523828,
            "range": "1977756.69–2000651.21",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12301184,
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
            "value": 140.41670299996622,
            "range": "128.44–166.66",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 128.43998199998168, 136.03805700002704, 140.41670299996622, 162.97156799997902, 166.66083299997263"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 504.7519480000483,
            "range": "444.20–541.98",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 444.2037840000121, 481.4821050000028, 504.7519480000483, 521.04813499999, 541.9849870000035"
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
          "id": "b14bff8e224e70ade237d4f215b26aa40b7ecd48",
          "message": "Merge pull request #287 from sims1253/fix/empty-json-output\n\nfix(cli): render empty reports when no R files are discovered",
          "timestamp": "2026-09-07T12:33:15+02:00",
          "tree_id": "96bbb647771ba4d0bfa64bd2750392564512d72b",
          "url": "https://github.com/sims1253/ry/commit/b14bff8e224e70ade237d4f215b26aa40b7ecd48"
        },
        "date": 1788777628299,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 11165456.381492984,
            "range": "11143795.07–11191908.00",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 1814727.1857407098,
            "range": "1812154.56–1817925.00",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5098147.3465782665,
            "range": "5050215.90–5160957.16",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 661759.5961507333,
            "range": "660894.45–662990.93",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 8411257.78365279,
            "range": "8386891.69–8445877.93",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1122882.7657442729,
            "range": "1119441.23–1127236.84",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 11608270.88658507,
            "range": "11384016.05–11974832.74",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 5634924.862639325,
            "range": "5622208.25–5654964.88",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1210731.4542833897,
            "range": "1209340.87–1212098.09",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2220691.6098806495,
            "range": "2217456.21–2224328.48",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 715799.1250150097,
            "range": "714173.13–717667.33",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 5633117.331481488,
            "range": "5623952.41–5646777.44",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1212050.6956998748,
            "range": "1210013.39–1214656.13",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 46799256.325,
            "range": "46748955.96–46853839.70",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 10293265.771540415,
            "range": "10273863.03–10313227.27",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 4672485.161003279,
            "range": "4635282.22–4708509.50",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 10359686.393409148,
            "range": "10312329.24–10412444.83",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4046536.013805984,
            "range": "4029537.21–4065377.99",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 11866517.083389463,
            "range": "11850327.78–11881261.93",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1926134.703395152,
            "range": "1921517.72–1930914.81",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12301648,
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
            "value": 109.00897700001951,
            "range": "93.13–117.52",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 93.13301900000079, 103.34549800003879, 109.00897700001951, 110.60187099999166, 117.52223899995442"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 433.7982779999729,
            "range": "400.71–485.92",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 400.7109170000185, 404.94970500000636, 433.7982779999729, 443.3373070000089, 485.91713300003903"
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
          "id": "4bb164c6ab390ddf92ea8d195c943883e95a6dd3",
          "message": "Merge pull request #286 from sims1253/fix/math-summary-fallback\n\nfix(checker): allow Math and Summary member fallback",
          "timestamp": "2026-09-07T12:46:56+02:00",
          "tree_id": "3e427598ca68e12d55c39e763df21d57bc77b111",
          "url": "https://github.com/sims1253/ry/commit/4bb164c6ab390ddf92ea8d195c943883e95a6dd3"
        },
        "date": 1788778332141,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 13302074.121723907,
            "range": "13275176.04–13338896.84",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2225827.7385761784,
            "range": "2223037.17–2228806.21",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5794330.543375695,
            "range": "5788606.65–5800745.33",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 827439.6472598761,
            "range": "826696.79–828295.58",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 9745325.906362357,
            "range": "9739017.99–9751958.56",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1350436.6735131652,
            "range": "1347785.46–1354200.77",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14404198.559132338,
            "range": "14255493.18–14629638.44",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7389876.642516759,
            "range": "7375873.62–7404692.04",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1497928.7104713616,
            "range": "1489890.41–1508547.48",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 3111997.2291676113,
            "range": "3109765.95–3114216.04",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 963836.3797811536,
            "range": "962826.82–965173.95",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7403739.874713315,
            "range": "7382216.00–7431669.53",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1486130.8027547994,
            "range": "1483988.97–1488853.48",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 69507604.36666667,
            "range": "69100298.83–69923208.79",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12862204.691564115,
            "range": "12776781.38–12980413.26",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5733292.82188137,
            "range": "5721469.39–5746308.50",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12878957.981815066,
            "range": "12852394.13–12902453.25",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5044007.337703716,
            "range": "4936411.53–5251172.24",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15229189.022974268,
            "range": "15129817.14–15360275.74",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1983968.4451494957,
            "range": "1975495.08–1992128.32",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12301624,
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
            "value": 134.83646999998018,
            "range": "126.57–163.31",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 126.57118400000036, 129.5858279997483, 134.83646999998018, 137.17289400007576, 163.31408400041983"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 482.53174700029194,
            "range": "472.33–539.13",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 472.3257849998772, 473.03534399997443, 482.53174700029194, 502.1958520002663, 539.1286049997434"
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
          "id": "8f53001df9f9c50e3c5d6ecbea5ad88151862da4",
          "message": "Merge pull request #289 from sims1253/fix/registered-s3-receiver\n\nfix(checker): require receiver evidence for registered S3 returns",
          "timestamp": "2026-09-07T13:07:35+02:00",
          "tree_id": "0fdb641c500903a3180ef07e0a6d6bd5be9e67ee",
          "url": "https://github.com/sims1253/ry/commit/8f53001df9f9c50e3c5d6ecbea5ad88151862da4"
        },
        "date": 1788779531312,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 9781848.125054736,
            "range": "9705226.05–9873591.91",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 1607457.2040726354,
            "range": "1573154.85–1647319.30",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 4482748.242073172,
            "range": "4435519.08–4530432.88",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 597886.298542305,
            "range": "582941.14–613891.03",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 7317412.255399553,
            "range": "7204910.83–7434676.39",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1018361.0496764483,
            "range": "997462.64–1038476.21",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 9596125.845633125,
            "range": "9513330.47–9715919.91",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 4857441.793055469,
            "range": "4764370.11–4964729.57",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1026120.0881056476,
            "range": "1014050.65–1043629.80",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 1881358.9366734952,
            "range": "1861117.16–1909580.79",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 604345.0781168133,
            "range": "595004.56–615654.87",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 4904896.7022735905,
            "range": "4824134.49–4989998.72",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1018498.9062473426,
            "range": "1014945.14–1022069.62",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 48514197.45,
            "range": "47514317.46–49555091.66",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 9192332.8775872,
            "range": "9077900.17–9330465.41",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 4148473.0415779115,
            "range": "4092184.48–4218942.75",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 9060881.443387808,
            "range": "8870373.27–9256976.27",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 3485626.8391959034,
            "range": "3419880.09–3561304.99",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 10629265.572584856,
            "range": "10582965.75–10676444.47",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1743221.1715865969,
            "range": "1726791.07–1760306.98",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12300680,
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
            "value": 85.78984399995534,
            "range": "81.37–92.49",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 81.36916800000472, 82.89719300001161, 85.78984399995534, 90.73328899999615, 92.49279899999965"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 401.70793899998534,
            "range": "390.53–475.52",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 390.52528599998914, 394.61593800003175, 401.70793899998534, 430.12186700000893, 475.51894900004845"
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
          "id": "bb525703e82f74c65e8f23a45726e7e5c4e0b821",
          "message": "Merge pull request #290 from sims1253/fix/scalar-parameter-guards\n\nfix(checker): respect scalar parameter guard result lengths",
          "timestamp": "2026-09-07T13:11:06+02:00",
          "tree_id": "cf67c1ce0704d1826e22c850b3e87e7b0c83b5ed",
          "url": "https://github.com/sims1253/ry/commit/bb525703e82f74c65e8f23a45726e7e5c4e0b821"
        },
        "date": 1788779818726,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 17038070.468856566,
            "range": "16398882.03–17666591.93",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2289625.2307658214,
            "range": "2286094.86–2293487.31",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 6088787.127528098,
            "range": "6067310.15–6114791.18",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 840823.8655336322,
            "range": "837061.58–846255.32",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 10030565.693924725,
            "range": "9995388.89–10071524.17",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1371698.2898356218,
            "range": "1357802.50–1395053.71",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14963308.747146424,
            "range": "14904478.61–15042594.59",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7034512.743479654,
            "range": "7005542.25–7078048.41",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1456524.8139284328,
            "range": "1455191.74–1458027.50",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 3046860.099616991,
            "range": "3044642.05–3049302.70",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 960631.6504534746,
            "range": "959714.69–961715.53",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7031849.800653835,
            "range": "7023431.07–7040846.04",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1456183.6205589497,
            "range": "1454076.88–1458484.66",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 75658387.5,
            "range": "75060670.34–76224728.35",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 13062044.228555974,
            "range": "13035416.50–13088146.43",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5784985.152978344,
            "range": "5742595.48–5839575.80",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 13241096.455305818,
            "range": "13211238.21–13270295.79",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5176452.40347718,
            "range": "5152798.62–5205500.83",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15662235.66952813,
            "range": "15565366.24–15779189.65",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1977818.227060326,
            "range": "1971052.26–1985053.02",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12303360,
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
            "value": 127.08742799999891,
            "range": "111.32–143.59",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 111.31988799996907, 118.11521999997785, 127.08742799999891, 136.63091200002236, 143.59334399999352"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 479.37019999994664,
            "range": "455.26–503.26",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 455.2554549999768, 468.5855539999902, 479.37019999994664, 484.93554099998437, 503.26063899998553"
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
          "id": "d24722292e3be0e392b1254a063ca934422f8407",
          "message": "Merge pull request #293 from sims1253/fix/ops-literal-identity\n\nProve incompatible Ops methods from unequal literal bodies",
          "timestamp": "2026-09-07T13:16:01+02:00",
          "tree_id": "4cb61b796b56433ae054a35f4a36b2cf1fa8b763",
          "url": "https://github.com/sims1253/ry/commit/d24722292e3be0e392b1254a063ca934422f8407"
        },
        "date": 1788780087990,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 11239937.362909932,
            "range": "11219996.47–11261182.79",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 1837711.6623212385,
            "range": "1826109.48–1850013.74",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5103322.9661201,
            "range": "5060585.52–5168143.28",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 669555.2011998731,
            "range": "667434.38–671943.95",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 8447792.724083029,
            "range": "8408869.60–8491866.95",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1164771.5816023399,
            "range": "1133220.85–1202528.95",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 11513910.100327762,
            "range": "11436798.78–11592008.62",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 5714950.630377235,
            "range": "5693050.15–5740480.18",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1236808.9119767768,
            "range": "1225498.96–1249393.83",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2268207.2265040176,
            "range": "2259672.54–2277409.81",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 723172.0622633802,
            "range": "720515.88–726161.23",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 5706544.201453799,
            "range": "5689239.17–5726841.22",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1214557.8074852233,
            "range": "1209540.27–1220358.56",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 49946461.425,
            "range": "49289220.81–50735234.27",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 10385092.19543606,
            "range": "10345285.15–10422523.09",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 4685588.563656371,
            "range": "4637809.44–4737051.76",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 10564438.854392413,
            "range": "10511315.84–10625275.92",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4230447.079829673,
            "range": "4197437.91–4264454.72",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 12063656.172267176,
            "range": "11935481.91–12262715.02",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1957757.710836844,
            "range": "1952237.24–1963598.56",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12304424,
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
            "value": 101.41500500001712,
            "range": "92.97–103.48",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 92.97044999996433, 98.73787700000685, 101.41500500001712, 101.90948099998059, 103.48110199999064"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 438.93901700002607,
            "range": "418.45–472.49",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 418.4515890000039, 419.51455999998143, 438.93901700002607, 450.16881699999794, 472.48587699997006"
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
          "id": "71d3a7628c79e34d506bedd4917ee4932c23a065",
          "message": "Merge pull request #285 from sims1253/fix/classed-atomic-dollar\n\nfix(checker): respect S3 dollar reads and writes on classed atomics",
          "timestamp": "2026-09-07T13:17:34+02:00",
          "tree_id": "83987d608ddeceb4490b15ca6ed64963894bc94a",
          "url": "https://github.com/sims1253/ry/commit/71d3a7628c79e34d506bedd4917ee4932c23a065"
        },
        "date": 1788780384212,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 13919625.150498047,
            "range": "13695756.83–14166410.05",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2246549.3666017405,
            "range": "2238814.46–2259733.39",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5861072.875562462,
            "range": "5855872.33–5866375.13",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 830133.05363586,
            "range": "828828.40–831728.24",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 9760082.752828928,
            "range": "9729001.91–9805787.17",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1340234.7377165179,
            "range": "1334064.33–1350717.25",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14575233.728916293,
            "range": "14525653.22–14640326.49",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7314294.295003504,
            "range": "7300733.34–7332965.77",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1508677.671734761,
            "range": "1504171.62–1513857.99",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 3120214.6733197477,
            "range": "3107133.21–3134216.50",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 978650.9128631391,
            "range": "972383.71–989991.74",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7310031.333253413,
            "range": "7301358.22–7318524.11",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1495705.6557545043,
            "range": "1493649.44–1498192.49",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 67926563.23333335,
            "range": "67556624.13–68314430.65",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 13021713.091855759,
            "range": "12961799.16–13098291.97",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5715017.284460617,
            "range": "5692271.02–5745845.69",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12950746.9537561,
            "range": "12900107.22–13009044.78",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5033274.068269601,
            "range": "5026042.37–5040112.20",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15377585.518047344,
            "range": "15289717.87–15495534.49",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1991594.6366299442,
            "range": "1980763.09–2000769.13",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12305176,
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
            "value": 128.78808600001503,
            "range": "117.75–139.21",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 117.75278099998832, 121.59116599999834, 128.78808600001503, 129.4181910000043, 139.20751500001643"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 471.46695899998304,
            "range": "460.57–483.09",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 460.5691549999756, 464.1874240000034, 471.46695899998304, 477.38976000004914, 483.0921269999817"
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
          "id": "56102c3dff90db647e637b8b92f1c4b74c66339b",
          "message": "Merge pull request #297 from sims1253/chore/sync-ggplot2-inventory\n\nEmbed verified ggplot2 inventory and correct polymorphic base returns",
          "timestamp": "2026-09-07T13:49:50+02:00",
          "tree_id": "bfb2af37f8ea3ea0712fb4daf545bf4162c44568",
          "url": "https://github.com/sims1253/ry/commit/56102c3dff90db647e637b8b92f1c4b74c66339b"
        },
        "date": 1788782104720,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 11844677.87184322,
            "range": "11744095.70–11959784.41",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 1957741.8275972486,
            "range": "1938571.58–1979926.78",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5572625.111088507,
            "range": "5519301.45–5635126.20",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 707697.6975931103,
            "range": "700989.24–714614.52",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 9081496.270025125,
            "range": "9008822.06–9153139.19",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1205403.5464724055,
            "range": "1190837.26–1218794.85",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 12738764.375906039,
            "range": "12602667.16–12896422.62",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6149239.774883477,
            "range": "6089563.31–6217159.67",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1361869.0015189436,
            "range": "1345220.09–1380307.03",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2407685.588710566,
            "range": "2388364.52–2427485.73",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 779556.2686739605,
            "range": "770105.81–788400.57",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6081392.248975318,
            "range": "6023433.12–6147039.91",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1311910.9524447531,
            "range": "1297522.32–1325608.78",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 48429786.61666666,
            "range": "48004182.70–48879676.70",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 11390162.131822173,
            "range": "11244824.58–11534991.84",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5026833.834615882,
            "range": "4943736.76–5115558.25",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 11358190.097627908,
            "range": "11266397.24–11439791.59",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4433280.203581678,
            "range": "4388031.77–4479466.30",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 13291662.325270358,
            "range": "13203883.22–13378212.85",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 2169061.401188923,
            "range": "2149098.85–2189356.08",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12770800,
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
            "value": 109.41811100003542,
            "range": "102.23–147.93",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 102.23339999996824, 109.34314599999925, 109.41811100003542, 115.05252600001404, 147.9254250000231"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 463.75516399997286,
            "range": "412.11–489.10",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 412.10578500002157, 435.5033120000153, 463.75516399997286, 482.2938270000159, 489.1032230000128"
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
          "id": "e4ee86c9a228eb07478ed820547a7d91451890c7",
          "message": "Merge pull request #301 from sims1253/fix/quoted-symbol-values\n\nResolve backtick-bound top-level values from ordinary reads",
          "timestamp": "2026-09-07T14:05:56+02:00",
          "tree_id": "2795ff926b65321a21956d6714c21d46dd471bbd",
          "url": "https://github.com/sims1253/ry/commit/e4ee86c9a228eb07478ed820547a7d91451890c7"
        },
        "date": 1788783059848,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 17325872.647134513,
            "range": "16639871.34–18089603.41",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2252763.2527258107,
            "range": "2244414.08–2264026.47",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 6198222.859912387,
            "range": "6099912.03–6316787.30",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 834577.7858736389,
            "range": "833083.23–836254.87",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 10316377.833086776,
            "range": "10130505.37–10528274.34",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1344906.4256682787,
            "range": "1342463.28–1348006.71",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14781355.869650293,
            "range": "14729458.27–14833253.79",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7494138.652310942,
            "range": "7471977.33–7520024.51",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1524585.1826476557,
            "range": "1507309.72–1552891.48",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 3144739.466351606,
            "range": "3137297.26–3154045.18",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 982851.6104483927,
            "range": "974727.72–996862.00",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7469927.542902455,
            "range": "7452921.25–7486878.03",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1497977.7485547045,
            "range": "1490595.51–1510789.47",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 73115879.98333332,
            "range": "72796099.20–73461124.24",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 13050710.87082302,
            "range": "12960691.33–13154418.52",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5738207.03913855,
            "range": "5709612.99–5777097.95",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 13270723.01421413,
            "range": "13069355.78–13538011.18",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5059804.312054196,
            "range": "5041811.22–5080828.67",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15600854.083218032,
            "range": "15476556.42–15750150.62",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 2006645.6942517362,
            "range": "1994245.54–2018202.66",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12771184,
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
            "value": 149.06840599997668,
            "range": "124.02–155.75",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 124.01586300000781, 140.3393700000015, 149.06840599997668, 150.02807599998778, 155.7540169999702"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 500.73871599999256,
            "range": "470.25–518.19",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 470.25037399999565, 499.9419160000398, 500.73871599999256, 507.5335399999749, 518.1889299999457"
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
          "id": "90403a42252a700015551aab78ff9a69bc8080fb",
          "message": "Merge pull request #296 from sims1253/fix/s4-initializer-provenance\n\ndocs(checker): establish S4 initializer proof groundwork",
          "timestamp": "2026-09-07T14:07:51+02:00",
          "tree_id": "0263a0f70131ce8ec0fe894b349447744c2686c7",
          "url": "https://github.com/sims1253/ry/commit/90403a42252a700015551aab78ff9a69bc8080fb"
        },
        "date": 1788783345233,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 15205950.261372011,
            "range": "14709920.34–15752822.68",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2233203.686999553,
            "range": "2228222.45–2239305.70",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 6005117.39887213,
            "range": "5955923.88–6056825.16",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 830345.6152374861,
            "range": "821351.23–844477.68",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 10103043.867079431,
            "range": "10037642.16–10171138.61",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1352327.0886915836,
            "range": "1349939.61–1355312.98",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14926402.478181932,
            "range": "14857663.65–15005701.94",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7440146.261708142,
            "range": "7428760.34–7451142.07",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1497143.9596096673,
            "range": "1489299.59–1510082.47",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 3116699.068587468,
            "range": "3104053.42–3131093.24",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 974380.2062492641,
            "range": "967840.13–983828.63",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7518858.790150712,
            "range": "7492567.17–7547712.48",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1493366.6455408751,
            "range": "1486238.38–1505170.54",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 75270700.54999998,
            "range": "74539111.39–76101807.59",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 13322169.998595584,
            "range": "13237337.47–13414882.98",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5746249.810568703,
            "range": "5728212.61–5766619.65",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 13346270.623439552,
            "range": "13263173.19–13433196.09",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5083498.155536221,
            "range": "5044051.01–5136098.31",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15737590.28232478,
            "range": "15632924.13–15856883.43",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1962060.8297465562,
            "range": "1950460.41–1975443.60",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12771184,
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
            "value": 134.69192899996415,
            "range": "112.27–168.82",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 112.2664929999737, 128.1959570000181, 134.69192899996415, 144.71226099994965, 168.821527000051"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 485.12194000009913,
            "range": "472.57–554.38",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 472.56822699995246, 473.46596599998884, 485.12194000009913, 517.2794559999602, 554.3828640000429"
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
          "id": "5889e226c5794fdcb9e7c9ee18e0fac94e80531a",
          "message": "Merge pull request #302 from sims1253/fix/rapply-matched-result\n\nRespect recursive apply result modes and retained classes",
          "timestamp": "2026-09-07T14:24:20+02:00",
          "tree_id": "046c56522d18c6e92803d55ea7b2193f5789593f",
          "url": "https://github.com/sims1253/ry/commit/5889e226c5794fdcb9e7c9ee18e0fac94e80531a"
        },
        "date": 1788784161050,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 21647344.457208093,
            "range": "21265416.37–22081991.82",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2312938.611747152,
            "range": "2306555.30–2321914.83",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 6876218.379368949,
            "range": "6707128.10–7062637.13",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 843543.3497951136,
            "range": "841349.58–846175.45",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 12703403.44391174,
            "range": "12355323.71–13072045.48",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1350439.649239148,
            "range": "1346983.03–1354247.89",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 15305071.964603644,
            "range": "15258325.55–15359902.27",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7110418.084358764,
            "range": "7086076.66–7137950.78",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1470619.730291762,
            "range": "1467168.52–1474173.54",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 3114961.2172974693,
            "range": "3096637.37–3137627.96",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 967964.8644533366,
            "range": "965464.02–970937.47",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7106654.963263278,
            "range": "7085976.51–7129124.13",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1471789.7129128608,
            "range": "1468360.87–1475740.73",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 83460253.4,
            "range": "83252842.95–83668250.89",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 14198077.66281785,
            "range": "14095631.83–14309133.81",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5823597.070811337,
            "range": "5801293.10–5851117.78",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 14177585.378081456,
            "range": "14138394.52–14218453.34",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5689494.2396288635,
            "range": "5625085.88–5764845.99",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 17284994.65569044,
            "range": "17038806.27–17566820.14",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 2070488.053217439,
            "range": "2045647.76–2099642.69",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12769952,
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
            "value": 149.75110699998913,
            "range": "138.31–161.71",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 138.3053030000301, 139.36531300004572, 149.75110699998913, 156.7079950000043, 161.70766499999445"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 498.9953540000133,
            "range": "474.83–510.24",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 474.82968399999663, 479.10113500000443, 498.9953540000133, 501.15148699999554, 510.24149300000863"
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
          "id": "259369c121599e495aa070109d5df6ef1f6c65cb",
          "message": "Merge pull request #298 from sims1253/fix/loop-exit-bindings\n\nPreserve bindings at loop breaks and next statements",
          "timestamp": "2026-09-07T14:25:39+02:00",
          "tree_id": "7530e031efbfd8cfe90204ee35fba1520e2e79e9",
          "url": "https://github.com/sims1253/ry/commit/259369c121599e495aa070109d5df6ef1f6c65cb"
        },
        "date": 1788784448497,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 14406705.245412815,
            "range": "14081501.67–14801702.04",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2218089.717218922,
            "range": "2211879.95–2224814.39",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 6141278.297090548,
            "range": "6083103.71–6205090.17",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 818353.0862397229,
            "range": "817087.03–819809.87",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 9876192.595805522,
            "range": "9844597.55–9910793.80",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1339367.349863676,
            "range": "1337660.16–1341557.99",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 15246049.098816538,
            "range": "14985012.86–15724812.29",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7419369.305673599,
            "range": "7405281.55–7437285.71",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1498600.9205909418,
            "range": "1483454.51–1520371.16",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 3131159.227828824,
            "range": "3120574.26–3140491.53",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 971207.0163307484,
            "range": "969011.19–973446.11",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7379713.445400229,
            "range": "7346483.57–7420175.46",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1477454.4130132855,
            "range": "1474471.39–1480996.22",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 74091693.15,
            "range": "73089673.55–75252405.15",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12989465.624973966,
            "range": "12940569.80–13039098.78",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5736035.80522546,
            "range": "5710494.35–5761323.13",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 13099379.343523905,
            "range": "13048830.66–13162567.75",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5038084.090728981,
            "range": "5009974.34–5080305.53",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15864022.787714565,
            "range": "15629153.35–16128290.92",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 2035654.6745142466,
            "range": "2028393.28–2043295.27",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12779472,
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
            "value": 129.38045900000725,
            "range": "121.85–144.13",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 121.84584999998333, 128.40922799997497, 129.38045900000725, 143.26634000003105, 144.12754600000335"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 489.82814400002826,
            "range": "461.73–544.62",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 461.7322249999852, 462.87386200000765, 489.82814400002826, 508.05502500000875, 544.6209580000141"
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
          "id": "146c29693b4ae1c5fa706f223601ba7e63cf1b4a",
          "message": "Merge pull request #300 from sims1253/feat/reference-blocker-provenance\n\nfeat(facts): expose primary reference blocker provenance",
          "timestamp": "2026-09-07T14:34:03+02:00",
          "tree_id": "fc0dfb2fd7dcf12ca3e728546b27576a09390e59",
          "url": "https://github.com/sims1253/ry/commit/146c29693b4ae1c5fa706f223601ba7e63cf1b4a"
        },
        "date": 1788784762145,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 14466602.786279257,
            "range": "14079199.11–14961688.88",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2232449.214917869,
            "range": "2226265.36–2240556.85",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5812910.629796918,
            "range": "5794813.91–5831760.07",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 814042.934462544,
            "range": "812891.26–815633.55",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 9866667.47849908,
            "range": "9832016.12–9906957.08",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1342273.696619226,
            "range": "1339612.89–1345589.58",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14982312.37649696,
            "range": "14774513.64–15245979.33",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7297262.683988646,
            "range": "7280527.30–7316770.36",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1577283.8293022923,
            "range": "1489773.43–1700463.38",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 3149871.3392926482,
            "range": "3143387.10–3156650.62",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 983834.6267174768,
            "range": "977826.13–992646.84",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7347228.946762649,
            "range": "7277150.14–7471695.94",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1488079.6647615107,
            "range": "1484244.30–1493265.91",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 75840697.275,
            "range": "75528202.58–76153672.67",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 13140301.961508093,
            "range": "13124267.34–13157662.99",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5784953.393571765,
            "range": "5742044.89–5847770.90",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 13171683.965281703,
            "range": "13137279.78–13211131.66",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5141441.379300846,
            "range": "5116883.45–5168537.37",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15767493.821578726,
            "range": "15661612.16–15910513.94",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1970490.8553784695,
            "range": "1960300.17–1980524.79",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12787664,
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
            "value": 132.20880000002217,
            "range": "112.13–144.61",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 112.13123100000666, 126.14921000000322, 132.20880000002217, 132.2482069999678, 144.61022500001127"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 470.93495399999665,
            "range": "464.42–503.59",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 464.41837200004375, 468.9532420000178, 470.93495399999665, 496.14966499997536, 503.5878480000538"
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
          "id": "7e04de24530fa169b5e36f2516661fad77bc58be",
          "message": "Merge pull request #305 from sims1253/perf/skip-empty-metadata-removals\n\nSkip hashing empty scope metadata tables during assignments",
          "timestamp": "2026-09-07T15:30:03+02:00",
          "tree_id": "26420033a537cf20fcecbb8b5fc47978a743ae8d",
          "url": "https://github.com/sims1253/ry/commit/7e04de24530fa169b5e36f2516661fad77bc58be"
        },
        "date": 1788788061678,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 8396210.404068729,
            "range": "8171665.75–8637372.23",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 1223720.4416624275,
            "range": "1211398.24–1237405.73",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 3255331.3415069263,
            "range": "3242193.72–3269430.71",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 464304.0016302676,
            "range": "460171.34–468917.13",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 5831636.297671122,
            "range": "5788760.27–5876827.76",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 835243.3975857073,
            "range": "795535.42–906858.36",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 7876344.415262932,
            "range": "7732280.03–8028647.34",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 3950412.0551644615,
            "range": "3852500.12–4105436.14",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 838239.7219030948,
            "range": "832836.82–844547.83",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 1329525.258071941,
            "range": "1322338.64–1337362.53",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 478617.07558510156,
            "range": "476708.82–480844.78",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 3826167.641046842,
            "range": "3815239.26–3838466.20",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 850939.8896404856,
            "range": "842904.20–859049.95",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 45072087.9875,
            "range": "44646197.63–45526483.54",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 6892892.614579691,
            "range": "6855403.02–6936166.27",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 3407794.356780874,
            "range": "3302541.21–3569796.90",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 6863086.644025363,
            "range": "6840895.65–6886109.61",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 2657162.5252330946,
            "range": "2638068.27–2681261.42",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 8384134.111104017,
            "range": "7981517.87–9013609.11",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1412425.2015724976,
            "range": "1402439.57–1423883.64",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12787728,
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
            "value": 83.67639999999665,
            "range": "59.74–88.25",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 59.7373770000122, 82.84346199998981, 83.67639999999665, 84.79940200000419, 88.24711400002707"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 377.10430799997994,
            "range": "358.99–452.36",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 358.98806900001364, 374.5778350000037, 377.10430799997994, 407.1220639999956, 452.36490899999626"
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
          "id": "91a361f966d9c37b23d0f5340113e817da8e6189",
          "message": "Merge pull request #299 from sims1253/feat/eager-reference-read\n\nCapture proven eager argument reads at the reference boundary",
          "timestamp": "2026-09-07T15:30:56+02:00",
          "tree_id": "8fc5eb36b1a8da66f4b16f7c927a88b8fc2827d7",
          "url": "https://github.com/sims1253/ry/commit/91a361f966d9c37b23d0f5340113e817da8e6189"
        },
        "date": 1788788350536,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 18239408.476841144,
            "range": "17382260.60–19144524.13",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2103086.055957543,
            "range": "2099940.98–2107613.76",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5626220.982845347,
            "range": "5573197.57–5683567.76",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 759688.4330146688,
            "range": "758614.91–760918.52",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 10170682.230385471,
            "range": "10066660.93–10301952.32",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1266409.5562680108,
            "range": "1264712.21–1268524.27",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 15263024.48378697,
            "range": "15103249.26–15497216.81",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6414037.626253192,
            "range": "6397257.87–6437023.16",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1379301.8148394371,
            "range": "1376245.68–1383329.20",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2519880.5770097924,
            "range": "2516522.82–2523602.42",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 879151.4501999412,
            "range": "877887.01–880738.22",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6439544.55674514,
            "range": "6423162.71–6459828.57",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1382161.89614061,
            "range": "1378525.26–1387024.87",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 79636088.425,
            "range": "79153675.39–80087678.45",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 14015710.491625965,
            "range": "13939852.29–14093714.32",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5782816.916448389,
            "range": "5757949.56–5813013.32",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 13913074.758207526,
            "range": "13860889.36–13966356.61",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5336954.8833942115,
            "range": "5287523.53–5393753.94",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 16523150.74414109,
            "range": "16426814.79–16628878.09",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1968874.8967969068,
            "range": "1960137.47–1980947.14",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12793904,
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
            "value": 133.81793100002687,
            "range": "128.71–157.89",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 128.71090399997775, 129.65669999999227, 133.81793100002687, 148.81129700003657, 157.88958900002763"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 497.1250059999875,
            "range": "470.15–559.72",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 470.1512999999686, 478.402808999992, 497.1250059999875, 535.4904000000097, 559.7243810000364"
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
          "id": "f6eefb2dfcb473783f85b7d4b2bd38b7ac4eb698",
          "message": "Merge pull request #306 from sims1253/fix/reduce-initial-accumulator\n\nfix(checker): avoid borrowing element types for fold accumulators",
          "timestamp": "2026-09-07T15:34:34+02:00",
          "tree_id": "1a066d679662b710ebc67aa8ecc907f81c055740",
          "url": "https://github.com/sims1253/ry/commit/f6eefb2dfcb473783f85b7d4b2bd38b7ac4eb698"
        },
        "date": 1788788650419,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 18183137.79152025,
            "range": "17770602.60–18625850.71",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2114641.5449123345,
            "range": "2097685.49–2143564.87",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5350543.380837156,
            "range": "5341580.10–5359274.92",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 762423.1113710682,
            "range": "755401.74–774473.34",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 9360442.836440247,
            "range": "9311597.43–9428281.80",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1273306.6781984873,
            "range": "1271618.83–1274954.42",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 15194468.163593452,
            "range": "15148950.78–15246151.97",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6492666.912945389,
            "range": "6469189.92–6512898.79",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1397506.8830619925,
            "range": "1389319.39–1408944.36",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2556438.213130706,
            "range": "2549664.20–2565190.11",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 899016.0921698611,
            "range": "895605.56–904148.76",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6496835.934295416,
            "range": "6459643.15–6542490.84",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1417206.8606277183,
            "range": "1390814.05–1456655.77",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 81721899.725,
            "range": "79321867.24–84977031.34",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 13204082.965195071,
            "range": "13176160.60–13229971.14",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5832952.180266655,
            "range": "5800282.61–5879408.43",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 13162906.873468582,
            "range": "13117838.01–13221994.18",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5192381.034721548,
            "range": "5179049.48–5206198.74",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15772106.30827115,
            "range": "15686077.92–15891292.95",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1965271.9519137207,
            "range": "1955802.44–1976402.07",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12796112,
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
            "value": 131.19894099998055,
            "range": "127.81–147.07",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 127.81269899994368, 130.87849400000414, 131.19894099998055, 132.48523099999875, 147.0723960000323"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 491.75002700003097,
            "range": "471.10–520.21",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 471.10217299999204, 473.9449120000354, 491.75002700003097, 492.6678209999809, 520.2107599999872"
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
          "id": "63e27635679143a8ddd514406140303ff7c4f595",
          "message": "Merge pull request #303 from sims1253/fix/storage-mode-replacement\n\nDiscard stale types after mode replacement assignments",
          "timestamp": "2026-09-07T15:58:17+02:00",
          "tree_id": "4aebc4cd8dcb191f44bb44f5d66e1cc155954d40",
          "url": "https://github.com/sims1253/ry/commit/63e27635679143a8ddd514406140303ff7c4f595"
        },
        "date": 1788789796784,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 10960793.882383702,
            "range": "10919919.82–11014028.91",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 1713159.5488940987,
            "range": "1701777.80–1731859.61",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 4811484.869365372,
            "range": "4741929.81–4926476.67",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 626935.6879561819,
            "range": "619348.76–637337.62",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 8167473.90187373,
            "range": "8113909.21–8256758.45",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1082073.070749447,
            "range": "1075513.59–1089655.22",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 11374236.747851197,
            "range": "11256339.80–11533317.10",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 5314976.464740997,
            "range": "5307090.94–5324621.61",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1181418.6272475286,
            "range": "1165047.63–1208048.87",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 1923089.7301139547,
            "range": "1911806.03–1940304.43",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 675609.7449449537,
            "range": "674868.75–676391.69",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 5331314.980546219,
            "range": "5324965.05–5338178.35",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1168013.525087023,
            "range": "1162800.10–1174054.39",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 43865245.1625,
            "range": "43759852.91–43991313.78",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 10105320.372437788,
            "range": "10059663.86–10169730.56",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 4622066.9439348765,
            "range": "4590557.58–4657727.79",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 10288288.510535588,
            "range": "10236051.60–10351741.01",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 3978832.331683539,
            "range": "3944253.00–4024284.28",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 11748579.671704615,
            "range": "11675189.79–11856386.12",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1950815.1660059982,
            "range": "1943193.69–1959439.36",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12796240,
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
            "value": 98.54772699996829,
            "range": "79.92–104.63",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 79.91542000003392, 83.84370100003434, 98.54772699996829, 102.46530099998927, 104.62960500002373"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 401.9550460000173,
            "range": "368.91–488.98",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 368.91270099999383, 396.2237099999911, 401.9550460000173, 417.34997400001157, 488.97784800000954"
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
          "id": "38a8eab42f2640ff451b28a05ae33a20512dced3",
          "message": "Merge pull request #307 from sims1253/fix/sync-aes-injection\n\nfix(typeshed): sync verified ggplot2 injection contracts",
          "timestamp": "2026-09-07T16:14:45+02:00",
          "tree_id": "87efb30792b41f88ae648eb7d50820b8f280a07f",
          "url": "https://github.com/sims1253/ry/commit/38a8eab42f2640ff451b28a05ae33a20512dced3"
        },
        "date": 1788790772882,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 15325259.052540436,
            "range": "14717655.56–15995410.64",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2094317.1579173026,
            "range": "2092272.96–2096772.77",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5564781.745756005,
            "range": "5539563.72–5596019.83",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 775851.4050633931,
            "range": "773978.66–778117.46",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 9628017.977707043,
            "range": "9574184.67–9687708.25",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1288008.4072310883,
            "range": "1286084.05–1289931.37",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14989093.69192194,
            "range": "14953302.66–15031414.84",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6504100.334203718,
            "range": "6491389.38–6516372.49",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1390225.0440112913,
            "range": "1379731.78–1408111.72",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2591939.692161372,
            "range": "2580495.51–2601527.99",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 899768.4848467888,
            "range": "895526.93–906477.20",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6542965.249099875,
            "range": "6509635.04–6590767.12",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1385887.5128214774,
            "range": "1384305.47–1387676.11",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 72183145.31666665,
            "range": "71575856.91–72779185.15",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 13525817.771731764,
            "range": "13446280.89–13632468.11",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5715946.669892414,
            "range": "5686431.18–5754781.16",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 13304014.411357138,
            "range": "13273617.65–13337179.96",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5257103.407089233,
            "range": "5231813.52–5294484.75",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 16211866.962603897,
            "range": "16112063.15–16346416.76",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1939442.5107755966,
            "range": "1936589.81–1942656.14",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12796496,
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
            "value": 135.44296399998711,
            "range": "120.36–164.43",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 120.36324100004276, 123.62623400002485, 135.44296399998711, 143.0023720000172, 164.42996000003768"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 496.8101489999681,
            "range": "460.60–514.22",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 460.59659500001, 474.9662900000112, 496.8101489999681, 502.2205379999941, 514.2226280000177"
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
          "id": "59ba64835f07a3d83536bc04ff25f71ec5471000",
          "message": "Merge pull request #309 from sims1253/chore/sync-base-contracts\n\nSync verified grep, fold, and confidence-interval contracts",
          "timestamp": "2026-09-07T16:29:02+02:00",
          "tree_id": "ebef414ce5e3f1a4b221659089766a5c9ec07b35",
          "url": "https://github.com/sims1253/ry/commit/59ba64835f07a3d83536bc04ff25f71ec5471000"
        },
        "date": 1788791684662,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 23845141.971640937,
            "range": "23480875.88–24212526.90",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2160134.4142809715,
            "range": "2121118.48–2200756.13",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 7426328.057726222,
            "range": "7277287.16–7572372.13",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 778155.7160001171,
            "range": "765706.15–793715.27",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 13035453.95847065,
            "range": "12754346.55–13316678.35",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1404290.5033736094,
            "range": "1375410.24–1433413.76",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 16095635.35196175,
            "range": "15768792.27–16438141.11",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6760874.022710597,
            "range": "6672549.58–6865124.11",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1509033.5038764789,
            "range": "1485678.73–1531174.97",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2433990.567648605,
            "range": "2388698.38–2481792.27",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 836302.6989602377,
            "range": "825244.53–848307.71",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7040724.295308584,
            "range": "6899611.74–7189619.11",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1484311.652456831,
            "range": "1457543.00–1512191.12",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 74825354.10000001,
            "range": "73507590.25–76461446.56",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 14470742.0790732,
            "range": "14200229.02–14749853.31",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5768723.2822985,
            "range": "5668626.40–5872093.36",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 14395587.588891754,
            "range": "14139468.80–14672679.25",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5835245.333658312,
            "range": "5716324.37–5964721.22",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 17457826.466040105,
            "range": "17093567.26–17898897.42",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 2740553.3269846463,
            "range": "2714803.98–2765267.10",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12796536,
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
            "value": 155.45234199997503,
            "range": "129.02–170.46",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 129.01919999998063, 129.90397899999516, 155.45234199997503, 164.2215299999807, 170.46137199999066"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 512.4224009999889,
            "range": "475.04–560.21",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 475.0423369999626, 507.0098869999638, 512.4224009999889, 549.4330259999842, 560.2062039999873"
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
          "id": "f64f0159e0cd5ffe26133345f816d8e455951d55",
          "message": "Merge pull request #312 from sims1253/fix/typeshed-sync-freshness\n\nfix(typeshed): rebuild embedded stubs after successful sync",
          "timestamp": "2026-09-07T16:34:09+02:00",
          "tree_id": "f1a4da86c469385d594e7a31d737fa80506da2ce",
          "url": "https://github.com/sims1253/ry/commit/f64f0159e0cd5ffe26133345f816d8e455951d55"
        },
        "date": 1788791991061,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 12881856.172030102,
            "range": "12860754.53–12905877.57",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2055627.3572592284,
            "range": "2052227.59–2060355.59",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5351026.871685059,
            "range": "5343757.54–5360048.44",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 769678.5698053536,
            "range": "759657.17–784567.22",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 9337177.758404901,
            "range": "9322358.21–9360192.11",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1277211.217379003,
            "range": "1274699.95–1280271.50",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14730122.62304524,
            "range": "14670718.44–14803442.37",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7011787.072135325,
            "range": "6996491.58–7028695.40",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1432978.3558215832,
            "range": "1425040.88–1444198.99",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2707509.92068035,
            "range": "2692539.05–2730557.10",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 915041.5895022124,
            "range": "913916.18–916309.72",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7003164.798860247,
            "range": "6990923.57–7013830.95",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1427824.477820481,
            "range": "1424295.28–1433188.08",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 68660425.54999998,
            "range": "68289864.40–69045112.13",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 13160204.808309803,
            "range": "13098290.72–13253292.73",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5772833.670581042,
            "range": "5744321.04–5809845.58",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 13236808.358761631,
            "range": "13200218.79–13288368.58",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5108258.8570997715,
            "range": "5097962.51–5118730.36",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15688788.94459865,
            "range": "15567027.40–15834409.76",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1951935.514941961,
            "range": "1940067.73–1963645.39",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12796536,
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
            "value": 142.98105200001737,
            "range": "131.26–170.21",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 131.2648160000099, 131.6768629999715, 142.98105200001737, 152.38446999998996, 170.21489699999802"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 496.78908100002445,
            "range": "479.07–514.72",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 479.07038499997, 479.841482000018, 496.78908100002445, 503.56428200000664, 514.7247039999929"
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
          "id": "af2b204a4fa20fc857e7b1ebf136404b12358c75",
          "message": "Merge pull request #310 from sims1253/chore/sync-expand-grid-contract\n\nfix(typeshed): correct expand.grid result contract",
          "timestamp": "2026-09-07T16:51:25+02:00",
          "tree_id": "d4ddf1cb5a56a9229a7e841d03d3159d98792bdc",
          "url": "https://github.com/sims1253/ry/commit/af2b204a4fa20fc857e7b1ebf136404b12358c75"
        },
        "date": 1788792966737,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 13798467.21755367,
            "range": "13628640.53–13978259.59",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2032664.6213706296,
            "range": "2029851.01–2035349.26",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5397440.3056722125,
            "range": "5387103.38–5407933.34",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 759728.1829775496,
            "range": "757752.14–762083.65",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 9544666.309067799,
            "range": "9499487.65–9594888.42",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1275274.6527265678,
            "range": "1269550.95–1283099.00",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14767935.484606836,
            "range": "14723223.73–14816343.95",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6886919.561227711,
            "range": "6868057.75–6907039.84",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1431603.7398057212,
            "range": "1420993.91–1449216.22",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2673071.663133594,
            "range": "2668701.35–2678282.81",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 910459.111553049,
            "range": "906186.89–917616.49",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6930390.758156012,
            "range": "6907682.11–6959030.49",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1416996.3461140702,
            "range": "1413225.96–1422455.95",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 69380354.46666667,
            "range": "69133193.90–69626896.96",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 13281765.622545378,
            "range": "13223464.21–13340913.89",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5774467.040778548,
            "range": "5737196.21–5819864.69",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 13324407.879515676,
            "range": "13292867.48–13358300.12",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5139397.085801536,
            "range": "5113556.71–5171774.56",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15749594.909351567,
            "range": "15658563.30–15868845.03",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1977391.9469534755,
            "range": "1966146.67–1989102.15",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12796624,
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
            "value": 141.85014299995964,
            "range": "126.33–160.42",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 126.3320459999959, 128.0930079999962, 141.85014299995964, 146.635895000014, 160.42289599997457"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 469.76559999998426,
            "range": "459.91–540.49",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 459.9102449999773, 461.94336600002134, 469.76559999998426, 486.8626010000007, 540.4850899999728"
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
          "id": "12518ac8190d498f2446fde7eb371e029827786f",
          "message": "Merge pull request #311 from sims1253/fix/forwarded-dots-arity\n\nfix(checker): account for forwarded dots in arity diagnostics",
          "timestamp": "2026-09-07T16:53:28+02:00",
          "tree_id": "dc0b4e5e7ed5dee20298cf64cb86e25770e9283b",
          "url": "https://github.com/sims1253/ry/commit/12518ac8190d498f2446fde7eb371e029827786f"
        },
        "date": 1788793266577,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 13746495.44086006,
            "range": "13562545.24–13977842.99",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 2088938.351363002,
            "range": "2086274.46–2092211.14",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5505740.953038646,
            "range": "5468455.84–5561099.78",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 761881.202020673,
            "range": "760333.26–764027.89",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 9476021.210949814,
            "range": "9442049.69–9517939.72",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1281782.287695982,
            "range": "1278733.35–1285565.15",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14918913.681084374,
            "range": "14890838.19–14956143.94",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6435002.744414432,
            "range": "6430551.08–6439228.34",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1384758.4079087055,
            "range": "1374730.10–1402199.99",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2511381.523989713,
            "range": "2509181.22–2513594.60",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 882768.9413916478,
            "range": "880570.04–885633.75",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6493893.35195164,
            "range": "6479280.58–6512905.47",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1387037.195905511,
            "range": "1382035.50–1394837.30",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 71568348.61666666,
            "range": "70705059.23–72476416.73",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 13127578.8421341,
            "range": "13102514.07–13153386.31",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5696546.52080153,
            "range": "5682983.42–5712728.15",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 13189946.673330106,
            "range": "13148830.91–13232985.67",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5112070.817211003,
            "range": "5089890.77–5134680.86",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15825380.256523594,
            "range": "15603659.84–16192872.70",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1954761.7191879437,
            "range": "1950972.15–1959220.81",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12798808,
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
            "value": 138.11806700000307,
            "range": "132.18–148.19",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 132.17518399999244, 134.0253139999695, 138.11806700000307, 143.51880000001984, 148.1896059999708"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 476.03106200002367,
            "range": "461.55–510.74",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 461.5506620000233, 475.9144850000157, 476.03106200002367, 496.4239499999676, 510.7449880000204"
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
          "id": "373d6a598a4a5dada6ba3551fd93f24b0e4dc9aa",
          "message": "Merge pull request #313 from sims1253/chore/sync-model-extract-contract\n\nSync model.extract component capture contract",
          "timestamp": "2026-09-07T17:16:33+02:00",
          "tree_id": "21d7df842092ef03541becf575f6ed6373f3427f",
          "url": "https://github.com/sims1253/ry/commit/373d6a598a4a5dada6ba3551fd93f24b0e4dc9aa"
        },
        "date": 1788794502750,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 18308664.095533475,
            "range": "17858835.62–18786332.32",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 1590770.9042139899,
            "range": "1588631.84–1593170.01",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5569534.619667383,
            "range": "5353666.58–5803660.81",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 596549.2564133453,
            "range": "595843.49–597393.19",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 10017300.03674219,
            "range": "9904403.04–10136991.40",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1003173.2583363357,
            "range": "999699.46–1008262.75",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 11004150.800119085,
            "range": "10905486.02–11104165.66",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 5073393.099595004,
            "range": "5048820.54–5096816.91",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1080641.8454739857,
            "range": "1072737.49–1089305.30",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 1966916.7201334618,
            "range": "1961552.42–1972931.78",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 680944.580859182,
            "range": "680017.75–682079.79",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 5124140.412146367,
            "range": "5112194.36–5136829.32",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1062667.8342923827,
            "range": "1059196.21–1068428.51",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 66450006.41666665,
            "range": "66068930.31–66898068.42",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 10187173.900337035,
            "range": "10135954.56–10243532.99",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 4455675.6727837445,
            "range": "4436913.62–4476431.87",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 10041593.939157736,
            "range": "9989708.72–10105678.13",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 3896122.543595367,
            "range": "3881920.13–3910508.10",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 12055124.599534962,
            "range": "11999141.98–12129158.60",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1535324.82330406,
            "range": "1529008.86–1540907.95",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12799160,
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
            "value": 100.80960799998138,
            "range": "83.77–108.57",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 83.76763799996115, 95.09423300001072, 100.80960799998138, 103.84601999999722, 108.56864599999972"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 405.6775450000423,
            "range": "397.65–582.23",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 397.64713599998504, 405.20684399997117, 405.6775450000423, 431.9198520000209, 582.2253839999903"
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
          "id": "0ec7bd51efbe426cd8d7a2f543effcf8a16a75a6",
          "message": "Merge pull request #308 from sims1253/perf/journal-statement-scopes\n\nperf(scope): journal statement branches instead of cloning scopes",
          "timestamp": "2026-09-07T17:21:14+02:00",
          "tree_id": "cf915fe01c36c9b075f59bb3ebbabcf195d9b16c",
          "url": "https://github.com/sims1253/ry/commit/0ec7bd51efbe426cd8d7a2f543effcf8a16a75a6"
        },
        "date": 1788794810219,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1660629.7255592388,
            "range": "1659404.21–1662205.25",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 602000.5609897735,
            "range": "600942.62–603281.88",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5866243.636107372,
            "range": "5754149.31–6017472.60",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 774906.6916037232,
            "range": "772140.80–778760.56",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 10121602.26798548,
            "range": "9934162.14–10355759.16",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1292109.7064143317,
            "range": "1289624.18–1295013.07",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 13828077.316980895,
            "range": "13802926.76–13855281.05",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6473514.471161703,
            "range": "6451744.49–6502230.57",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1377257.1337597482,
            "range": "1375702.35–1378999.22",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2536432.6476643765,
            "range": "2523649.80–2554227.57",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 887240.1990965668,
            "range": "885507.88–889478.29",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6495212.4471792,
            "range": "6476174.45–6514880.46",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1383537.9505070446,
            "range": "1378062.90–1392792.03",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 75997740.525,
            "range": "74678417.27–77587504.32",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12813753.774575384,
            "range": "12741402.76–12892664.69",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5684402.895375115,
            "range": "5661696.27–5722559.95",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12462109.79219862,
            "range": "12417807.54–12509094.94",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4965985.877776397,
            "range": "4939996.61–4994653.66",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15254965.355980888,
            "range": "15149100.02–15401413.93",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1998708.7604135678,
            "range": "1981551.62–2016288.31",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12841120,
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
            "value": 146.28012200002559,
            "range": "115.34–159.65",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 115.33930200000759, 138.5517600000021, 146.28012200002559, 158.42858099995647, 159.64770600001793"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 503.9094729999779,
            "range": "444.73–520.74",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 444.7317879999755, 494.1673979999614, 503.9094729999779, 503.91259199997876, 520.7443610000191"
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
          "id": "36d6103987d8b0f1db010b6f6453b80e80381680",
          "message": "Merge pull request #314 from sims1253/chore/sync-html-tags-contract\n\nfix(typeshed): resolve exported HTML tag lists",
          "timestamp": "2026-09-07T17:42:38+02:00",
          "tree_id": "797cd8b3fae40a3056ce455dadd6e6ffe9d03f03",
          "url": "https://github.com/sims1253/ry/commit/36d6103987d8b0f1db010b6f6453b80e80381680"
        },
        "date": 1788796059412,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1649564.7764057084,
            "range": "1646551.97–1654471.27",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 593572.1894419824,
            "range": "592818.51–594504.58",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 6019801.592786095,
            "range": "5972383.17–6081727.69",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 764491.5194397368,
            "range": "762799.98–767029.65",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 10654901.075386534,
            "range": "10507531.67–10810093.12",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1287926.594729366,
            "range": "1286246.96–1289765.33",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14250092.055427644,
            "range": "14166871.35–14361244.56",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6492847.909355021,
            "range": "6481009.18–6505220.39",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1385110.2828245838,
            "range": "1374114.37–1404222.31",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2500982.408690512,
            "range": "2497165.31–2505223.43",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 887098.6377622385,
            "range": "885643.09–889141.58",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6507966.0203112215,
            "range": "6498480.58–6518713.98",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1373224.195392657,
            "range": "1371075.14–1375545.97",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 75967736.225,
            "range": "75757679.58–76176484.82",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12565895.713959018,
            "range": "12517370.45–12616032.91",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5659832.4762493,
            "range": "5647998.40–5673268.37",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12979572.37001803,
            "range": "12933723.10–13022684.96",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5008058.779162077,
            "range": "4992455.86–5026001.91",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15124632.026966343,
            "range": "14994409.92–15308121.76",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1954083.4831966036,
            "range": "1949566.48–1958982.19",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12842456,
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
            "value": 137.02613200002816,
            "range": "121.53–158.12",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 121.52594399999361, 122.4884440000169, 137.02613200002816, 137.83504599996377, 158.11611099995207"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 487.5244380000513,
            "range": "466.22–541.92",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 466.2170140000526, 467.2320530000143, 487.5244380000513, 490.78653799998574, 541.923681999906"
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
          "id": "6f74c557f4f783df9e141e8b0f61b3d75acc6b7c",
          "message": "Merge pull request #315 from sims1253/chore/sync-filter-position-contracts\n\nfix(typeshed): preserve unknown Filter and Position results",
          "timestamp": "2026-09-07T17:45:08+02:00",
          "tree_id": "829ccf0d99ede38434e8c9f9be8a935b27b3f350",
          "url": "https://github.com/sims1253/ry/commit/6f74c557f4f783df9e141e8b0f61b3d75acc6b7c"
        },
        "date": 1788796338675,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1579735.6125238573,
            "range": "1576391.00–1583212.71",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 576877.7292809942,
            "range": "576429.89–577350.69",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5285034.174419635,
            "range": "5263680.13–5312610.22",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 748561.273839367,
            "range": "747859.63–749461.53",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 9192979.52251447,
            "range": "9185342.48–9201053.42",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1263619.5346971077,
            "range": "1262294.33–1265045.87",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14001307.758559447,
            "range": "13937906.46–14061493.11",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6826754.486258988,
            "range": "6777731.66–6903933.99",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1413386.9301513985,
            "range": "1411218.24–1415897.64",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2541947.3354550526,
            "range": "2537857.57–2546813.63",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 895550.46680159,
            "range": "894459.31–897043.50",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6769320.315903196,
            "range": "6761825.89–6778750.63",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1414025.2111443859,
            "range": "1409141.95–1420707.46",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 66418320.96666666,
            "range": "66076253.77–66794842.27",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12463570.738779008,
            "range": "12406305.34–12551162.76",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5668742.0169253405,
            "range": "5650649.87–5686392.43",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12433867.074717434,
            "range": "12406313.21–12465539.53",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4900482.8476498965,
            "range": "4886885.17–4915242.04",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 14947133.74222065,
            "range": "14840241.78–15102929.24",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1925530.1758506305,
            "range": "1917874.20–1935709.57",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12842488,
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
            "value": 148.42505800002255,
            "range": "121.29–173.69",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 121.28676599997561, 146.72298500000034, 148.42505800002255, 171.74699300003704, 173.68746100005228"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 497.142090999987,
            "range": "446.64–533.46",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 446.6370319999987, 491.90485300001455, 497.142090999987, 525.3031480000354, 533.4613890000037"
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
          "id": "3afc559bb146f5c9a56b28d514060c43ed720665",
          "message": "Merge pull request #316 from sims1253/fix/distinguish-slot-extraction\n\nfix(checker): distinguish slot extraction from dollar access",
          "timestamp": "2026-09-07T18:13:44+02:00",
          "tree_id": "a12711414e2fc893422eac4306fb22c5a1136435",
          "url": "https://github.com/sims1253/ry/commit/3afc559bb146f5c9a56b28d514060c43ed720665"
        },
        "date": 1788798161516,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1686441.7800552938,
            "range": "1675048.87–1707039.34",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 606634.7493730313,
            "range": "605915.35–607566.57",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5831797.994658811,
            "range": "5781058.72–5900378.61",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 772349.3393518778,
            "range": "770930.63–774352.00",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 10216602.697311722,
            "range": "10048746.74–10487271.75",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1281335.648000368,
            "range": "1279778.47–1283145.03",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14636283.423160315,
            "range": "14475292.59–14878986.32",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6506235.623624744,
            "range": "6500105.50–6512458.52",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1418992.716665682,
            "range": "1397068.35–1451372.60",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2560919.0692665377,
            "range": "2557142.19–2565509.91",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 896874.6971274323,
            "range": "895060.68–899097.34",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6513571.050503908,
            "range": "6505383.67–6523421.66",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1387883.6985027173,
            "range": "1384548.24–1392102.58",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 85254054.325,
            "range": "84770222.72–85779078.29",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12981753.645853953,
            "range": "12948437.61–13014076.73",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5784692.598096041,
            "range": "5773176.88–5795354.76",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 13011218.47025686,
            "range": "12969581.82–13058941.95",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4987705.882536758,
            "range": "4962926.78–5013571.85",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15961381.942015326,
            "range": "15669250.21–16369071.61",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1970582.229064205,
            "range": "1961999.58–1979157.01",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12841360,
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
            "value": 130.4017660000245,
            "range": "112.33–138.00",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 112.32508000003872, 122.08859600004507, 130.4017660000245, 136.31043700000737, 137.99841700005345"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 479.5803850000375,
            "range": "433.48–489.89",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 433.4774590000161, 465.9688020000467, 479.5803850000375, 486.28691600001184, 489.89325399999507"
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
          "id": "d359ce63122234d34a1319e21f1abc97b64cc461",
          "message": "Merge pull request #325 from sims1253/t3code/upstream-treesitter-changes\n\nchore(parser): use the published R grammar",
          "timestamp": "2026-09-07T19:58:00+02:00",
          "tree_id": "5e147945dd111b5ab6743ee1dfa2c031091f8b04",
          "url": "https://github.com/sims1253/ry/commit/d359ce63122234d34a1319e21f1abc97b64cc461"
        },
        "date": 1788804206324,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1618195.2204851743,
            "range": "1615953.89–1620701.94",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 597242.4331256208,
            "range": "596654.46–597901.66",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5311046.141338503,
            "range": "5288183.41–5336447.47",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 764778.9805081164,
            "range": "762676.24–767450.97",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 9366814.246786049,
            "range": "9294778.46–9469615.46",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1280246.995207581,
            "range": "1274722.61–1288955.98",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14027333.710250124,
            "range": "13905247.89–14213282.46",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6876187.469580263,
            "range": "6864604.48–6889550.89",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1436761.2527237558,
            "range": "1433984.36–1440202.77",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2660773.32629114,
            "range": "2657035.94–2665782.62",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 924950.4623718507,
            "range": "914800.31–939641.38",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6867117.806763887,
            "range": "6850701.90–6889244.08",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1428232.1654768842,
            "range": "1426623.54–1430142.41",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 71205525.51666667,
            "range": "70855617.22–71566137.05",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12291934.586292828,
            "range": "12260935.47–12324891.31",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5752971.458445957,
            "range": "5740174.94–5766182.15",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12425725.172542796,
            "range": "12368862.59–12489184.25",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4839074.255948227,
            "range": "4831621.20–4848939.76",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 14868316.542226857,
            "range": "14741044.05–15026551.79",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1987145.4783790756,
            "range": "1982400.99–1992120.42",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12840048,
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
            "value": 136.289775000012,
            "range": "114.05–175.46",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 114.05006799998228, 121.76597000000766, 136.289775000012, 147.87464100000216, 175.45668599999044"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 485.30359299998963,
            "range": "447.54–518.33",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 447.54333499999484, 476.40424399997573, 485.30359299998963, 514.2762169999769, 518.3285789999645"
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
          "id": "afd63989b57c00bce911813592d7988ad489faf8",
          "message": "refactor(vscode): use Effect for commands and server lifecycle (#327)\n\n* refactor(vscode): use Effect for commands and server lifecycle\n\n* fix(vscode): preserve failure details through Effect wrappers",
          "timestamp": "2026-09-07T20:54:22+02:00",
          "tree_id": "77bf966adc42123aed888f8a23879c052ce7e89a",
          "url": "https://github.com/sims1253/ry/commit/afd63989b57c00bce911813592d7988ad489faf8"
        },
        "date": 1788807551980,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1674044.6460691046,
            "range": "1672000.59–1676818.60",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 595504.0985338155,
            "range": "594679.93–596581.62",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5531240.855811992,
            "range": "5522338.49–5541566.74",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 772067.1440929286,
            "range": "770663.08–773740.10",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 9438477.300660409,
            "range": "9430540.88–9446103.93",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1291553.1152152224,
            "range": "1289863.31–1293606.66",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14181247.163466627,
            "range": "14163794.50–14200121.58",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6580587.310977003,
            "range": "6574766.70–6587856.56",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1384673.8999390146,
            "range": "1381433.20–1389037.78",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2608271.02877824,
            "range": "2605993.96–2610536.13",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 903692.835936673,
            "range": "898168.20–912512.29",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6674286.519990737,
            "range": "6641203.57–6735440.36",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1387186.8864118783,
            "range": "1385774.67–1388759.23",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 69855368.15,
            "range": "69787138.98–69926878.17",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12397126.507353093,
            "range": "12377996.08–12416791.90",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5667734.26817928,
            "range": "5648315.55–5693548.24",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12530407.669197384,
            "range": "12491076.46–12571590.68",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4882125.148598639,
            "range": "4878051.17–4886132.35",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15025277.810769415,
            "range": "14899236.84–15184471.89",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1957362.5155499135,
            "range": "1954194.22–1960750.44",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12840048,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 1713883,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 303989,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 205.5216060000239,
            "range": "197.83–256.04",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 197.8295999999973, 198.1150519999792, 205.5216060000239, 208.82902800000738, 256.0430040000356"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 551.6745339999907,
            "range": "528.98–612.71",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 528.9836589999613, 539.0262559999828, 551.6745339999907, 570.2973249999923, 612.7071529999957"
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
          "id": "4f699791f420fba233ad97e4ae48ca45dfaab096",
          "message": "refactor(cli): simplify project setup and correct release guidance (#328)",
          "timestamp": "2026-09-07T20:57:52+02:00",
          "tree_id": "e2208832ce7e10cf9ab7cf4793a87e6a32f9cd67",
          "url": "https://github.com/sims1253/ry/commit/4f699791f420fba233ad97e4ae48ca45dfaab096"
        },
        "date": 1788807836954,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1647692.716947894,
            "range": "1643940.81–1651795.73",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 602690.057315495,
            "range": "602025.62–603493.61",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5438453.670791871,
            "range": "5409040.44–5483862.53",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 773137.4822094648,
            "range": "770118.68–777063.28",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 9393268.744225891,
            "range": "9370287.58–9422516.97",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1286440.5132878604,
            "range": "1283313.20–1290974.73",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14077834.596750434,
            "range": "14017834.99–14140207.93",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7010705.575415544,
            "range": "6970386.68–7074782.63",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1443093.789937251,
            "range": "1440308.39–1446244.61",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2671162.876254362,
            "range": "2667576.18–2675883.56",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 922273.7642921644,
            "range": "917557.05–928856.99",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6980233.918298152,
            "range": "6957259.81–7010728.08",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1426890.814055488,
            "range": "1424833.16–1429444.63",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 75102203.575,
            "range": "74738250.91–75443510.51",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12436798.21104112,
            "range": "12386662.94–12487439.37",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5761067.479239584,
            "range": "5727562.41–5795755.73",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12495080.25003732,
            "range": "12385829.81–12662890.70",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4880123.491212799,
            "range": "4835307.51–4946558.77",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15207845.544627223,
            "range": "15077906.81–15381801.50",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1988663.415650819,
            "range": "1971314.23–2005576.04",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12826024,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 1713883,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 303989,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 202.94563600001857,
            "range": "195.22–214.47",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 195.22157599998172, 201.48167999996804, 202.94563600001857, 209.54187399998773, 214.4744130000472"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 570.3430040000239,
            "range": "544.20–574.41",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 544.1973470000084, 561.4223579999525, 570.3430040000239, 572.8722889999626, 574.4077929999912"
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
          "id": "4ea9962115a8dd71b150a8ae8ea2b6a65f5aa99a",
          "message": "chore(editors): publish as scholzmx.ry (#329)\n\n* chore(editors): publish as scholzmx.ry\n\n* fix(editors): finish scholzmx rename in performance test and CI guard\n\n* docs(runbook): sync publisher identity gate files",
          "timestamp": "2026-09-07T21:58:03+02:00",
          "tree_id": "e658c79403b2dfcb739e494338e13151737bc7f2",
          "url": "https://github.com/sims1253/ry/commit/4ea9962115a8dd71b150a8ae8ea2b6a65f5aa99a"
        },
        "date": 1788811331590,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1316073.26244403,
            "range": "1315017.50–1317144.41",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 465579.2071675634,
            "range": "465094.19–466083.87",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 4294839.57240277,
            "range": "4287437.12–4301436.05",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 600402.0232827042,
            "range": "599721.74–601196.08",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 7391548.300244195,
            "range": "7374915.65–7410253.91",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1007305.1901754586,
            "range": "1006362.22–1008261.70",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 10113446.274901176,
            "range": "10024109.13–10211946.20",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 5061024.163144348,
            "range": "5056000.66–5066247.10",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1082078.7703511766,
            "range": "1080753.24–1083797.45",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2035094.2905412428,
            "range": "2033555.81–2036805.28",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 700249.5802857695,
            "range": "699100.71–702024.08",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 5074367.6688976865,
            "range": "5068747.84–5082564.20",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1085549.1252584564,
            "range": "1084624.02–1086442.33",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 59105493.05,
            "range": "58659565.12–59625343.67",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 9056614.081341412,
            "range": "9047843.45–9067029.17",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 4390996.122800264,
            "range": "4381890.60–4398988.44",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 9045690.819236925,
            "range": "9033992.59–9057856.82",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 3536491.517712849,
            "range": "3519881.43–3553317.46",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 10872485.679011034,
            "range": "10846855.67–10904933.31",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1555468.7161830666,
            "range": "1546816.72–1564210.98",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12826024,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 1713883,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 304002,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 150.8060359999945,
            "range": "143.23–164.18",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 143.22686299998895, 146.71128700001282, 150.8060359999945, 157.19834299999638, 164.17593900000793"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 473.7358660000027,
            "range": "442.62–547.31",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 442.6228280000214, 465.43833299999824, 473.7358660000027, 493.7797600000049, 547.3077489999996"
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
          "id": "86f9c5eeca923f021b6fe00e7bfa95db771200d5",
          "message": "Merge pull request #330 from sims1253/fix/release-simplification\n\nFix apply-family simplification and empty-input results",
          "timestamp": "2026-09-07T22:06:11+02:00",
          "tree_id": "9499fd68554226a92525401632e1208bb302e91c",
          "url": "https://github.com/sims1253/ry/commit/86f9c5eeca923f021b6fe00e7bfa95db771200d5"
        },
        "date": 1788811858609,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1530619.0630371517,
            "range": "1527349.66–1534850.87",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 545816.1606327079,
            "range": "537402.95–561566.54",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5228835.242077312,
            "range": "5219149.17–5238690.69",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 730355.6938236811,
            "range": "728721.57–732351.57",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 8968923.050992051,
            "range": "8938914.61–8998662.54",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1237635.0182039682,
            "range": "1234960.25–1240899.30",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 12408616.382366464,
            "range": "12305815.97–12537449.21",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6097741.444654252,
            "range": "6066922.20–6124983.86",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1349042.0260901747,
            "range": "1340231.84–1360188.01",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2349337.644317478,
            "range": "2342729.15–2357939.75",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 840076.5817299022,
            "range": "837395.25–843433.69",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6092695.550191415,
            "range": "6063252.39–6117765.17",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1344211.5817956452,
            "range": "1340500.79–1347562.49",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 70861465.43333334,
            "range": "70487692.12–71353741.62",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 10816674.057887342,
            "range": "10783658.27–10856739.45",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5510166.738418887,
            "range": "5490912.88–5527119.14",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 11172028.916500468,
            "range": "11108385.91–11240774.55",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4227571.79751953,
            "range": "4205964.14–4252703.96",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 12996239.67108777,
            "range": "12928382.22–13076404.02",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 2230915.3169987057,
            "range": "2221576.28–2240779.64",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12827904,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 1713883,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 304002,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 186.4435119999689,
            "range": "178.29–197.75",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 178.28870199999074, 183.43671999999788, 186.4435119999689, 188.59042800002499, 197.74736699997447"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 520.7865250000032,
            "range": "491.09–526.16",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 491.09217600000557, 495.92861699999776, 520.7865250000032, 522.291194999998, 526.1640129999723"
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
          "id": "7c3f07b8bb72c201ed76a965e76a0d3127b23fa8",
          "message": "Merge pull request #331 from sims1253/fix/release-grep-length\n\nfix(checker): preserve valid grep-length boolean idioms",
          "timestamp": "2026-09-07T22:20:14+02:00",
          "tree_id": "bd0df71167d76c2f95d6dcd9f378cfea5e6f3287",
          "url": "https://github.com/sims1253/ry/commit/7c3f07b8bb72c201ed76a965e76a0d3127b23fa8"
        },
        "date": 1788812707643,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1590885.3886447444,
            "range": "1588233.28–1593890.30",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 584850.428836036,
            "range": "584088.96–585693.44",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5414818.152791506,
            "range": "5396517.33–5432527.94",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 777293.6737355359,
            "range": "776431.81–778176.56",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 9622016.981407229,
            "range": "9573443.61–9677408.96",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1303414.879143766,
            "range": "1302010.23–1304938.12",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14216471.980196532,
            "range": "14174802.37–14267733.99",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6964271.195023659,
            "range": "6957237.94–6971635.08",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1446181.544908994,
            "range": "1443805.26–1449107.52",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2642096.2632803405,
            "range": "2637649.87–2647661.03",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 930112.1559577103,
            "range": "918237.14–945541.14",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6988688.798156723,
            "range": "6968386.03–7015672.75",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1437668.6423497205,
            "range": "1435175.68–1440923.18",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 72734636.11666667,
            "range": "72459225.03–73044714.42",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12757568.54157503,
            "range": "12638354.58–12952181.90",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5777044.051014159,
            "range": "5755579.15–5804664.08",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12569775.780002285,
            "range": "12542754.36–12600874.23",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4962065.403492968,
            "range": "4947395.45–4978758.45",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15095151.610022565,
            "range": "14998097.18–15225836.87",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 2006123.375447988,
            "range": "1993974.71–2023526.34",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12833256,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 1713883,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 304002,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 217.87404999998398,
            "range": "207.52–238.34",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 207.51694200001657, 208.96955999999773, 217.87404999998398, 219.17333000001963, 238.33559900004184"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 584.2885310000274,
            "range": "541.43–614.49",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 541.4291750000557, 574.7138670000131, 584.2885310000274, 592.296285999997, 614.4905339999823"
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
          "id": "5605cd95e7cecf1876e23f58437df30e08e0c644",
          "message": "Merge pull request #332 from sims1253/fix/release-length-dispatch\n\nFix constant-length warnings for S3-dispatched values",
          "timestamp": "2026-09-07T22:25:50+02:00",
          "tree_id": "176c0a45825246989b5bf241b1cfb115186fd295",
          "url": "https://github.com/sims1253/ry/commit/5605cd95e7cecf1876e23f58437df30e08e0c644"
        },
        "date": 1788813016457,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1002695.6222489306,
            "range": "992857.95–1013238.85",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 354054.37880448304,
            "range": "353687.38–354545.58",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 4018925.568177648,
            "range": "3986141.94–4060752.38",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 521524.99161030195,
            "range": "521054.92–522088.89",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 6645107.287819571,
            "range": "6613334.63–6675639.61",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 906011.1286196944,
            "range": "900454.85–914586.71",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 8642752.85635519,
            "range": "8630172.58–8656806.73",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 4326406.727689177,
            "range": "4322768.04–4330578.60",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 947135.9011329934,
            "range": "945959.64–948927.70",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 1581903.9038868756,
            "range": "1570704.63–1596334.90",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 552452.0864663327,
            "range": "548080.88–558371.83",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 4328658.853213664,
            "range": "4324827.24–4332908.76",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 952920.8878791915,
            "range": "948988.40–957827.77",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 41269023.825,
            "range": "40561157.18–42116611.26",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 7900162.808376892,
            "range": "7837252.06–7994731.49",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 4033719.7933023362,
            "range": "3996415.04–4077939.52",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 7942929.063939951,
            "range": "7918723.27–7964602.09",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 3228703.4391135583,
            "range": "3214566.29–3241775.18",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 9264551.67585801,
            "range": "9189509.05–9364477.68",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1612387.1558379456,
            "range": "1585641.85–1646917.47",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12835368,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 1713883,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 304002,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 144.72182299999986,
            "range": "124.10–174.50",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 124.09604199999012, 127.94151299999794, 144.72182299999986, 154.89804200001527, 174.5019420000026"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 452.0998079999699,
            "range": "418.93–483.36",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 418.9267450000043, 425.48415699996985, 452.0998079999699, 482.1693099999684, 483.3554629999562"
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
          "id": "5f8d972e15a1048464b6a720e0c2b44c5e297022",
          "message": "Merge pull request #335 from sims1253/sims1253-patch-1\n\nEnhance README with emoji in important note",
          "timestamp": "2026-09-07T22:50:19+02:00",
          "tree_id": "0afbe0a463f76642cd06a5c4ea3535215d6cdab0",
          "url": "https://github.com/sims1253/ry/commit/5f8d972e15a1048464b6a720e0c2b44c5e297022"
        },
        "date": 1788814473152,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1286421.9659472136,
            "range": "1284982.85–1288447.47",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 463842.661076313,
            "range": "463290.26–464485.25",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 4354952.945955548,
            "range": "4332845.82–4377894.89",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 594591.1869904125,
            "range": "593570.39–595975.67",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 7438023.6636696905,
            "range": "7411368.65–7464058.19",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 997364.7852731964,
            "range": "993944.58–1001770.64",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 10115737.70779766,
            "range": "10013465.54–10228023.85",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 5082638.648285799,
            "range": "5073283.42–5092982.81",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1076277.5615688912,
            "range": "1070266.52–1084200.31",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2013838.8361654938,
            "range": "2009615.92–2018363.96",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 687556.350838722,
            "range": "685011.68–691651.20",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 5107699.742380518,
            "range": "5098880.61–5117950.63",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1074026.6067213272,
            "range": "1072600.24–1075771.83",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 63818618.84999999,
            "range": "62435756.35–65580033.72",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 8963652.96065988,
            "range": "8926140.74–9002654.24",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 4419228.115739835,
            "range": "4404670.90–4434637.71",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 8993574.692446467,
            "range": "8925663.84–9084622.22",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 3478333.23741908,
            "range": "3467618.08–3491295.32",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 10764617.715987144,
            "range": "10732925.62–10799898.99",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1512707.2316628185,
            "range": "1509668.34–1516306.99",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12835368,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 1713883,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 304002,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 158.3802550000255,
            "range": "155.65–186.94",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 155.65278499998385, 157.33570200001122, 158.3802550000255, 172.1488830000162, 186.9379140000092"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 478.30896799999755,
            "range": "456.38–535.61",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 456.38209100003587, 472.11595499998657, 478.30896799999755, 534.2543210000149, 535.610515999957"
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
          "id": "156ecfe830e4d64bb3fb4658199191239b6c2ca3",
          "message": "chore(release): prepare 0.9.0 (#336)",
          "timestamp": "2026-09-07T22:54:55+02:00",
          "tree_id": "9eef97785c935a288a34233f47636536fc8857ac",
          "url": "https://github.com/sims1253/ry/commit/156ecfe830e4d64bb3fb4658199191239b6c2ca3"
        },
        "date": 1788814786489,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1662366.3948353499,
            "range": "1660645.12–1664865.81",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 603999.14523511,
            "range": "603160.61–605185.02",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5471347.192357215,
            "range": "5465649.68–5477102.28",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 766117.558041247,
            "range": "765418.96–766823.28",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 9419003.878257114,
            "range": "9409597.40–9432252.27",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1309147.033177242,
            "range": "1282804.39–1347968.68",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14146104.025087843,
            "range": "14114400.22–14178575.33",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6504303.586114231,
            "range": "6499356.50–6509193.98",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1396704.0632645772,
            "range": "1385637.18–1417090.17",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2563474.10803918,
            "range": "2561212.55–2566006.91",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 896499.7663320473,
            "range": "895097.06–898120.83",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6532853.555015916,
            "range": "6525154.58–6543889.00",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1393042.709373137,
            "range": "1381047.83–1410411.05",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 71556572.96666667,
            "range": "71501198.96–71615724.27",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12584533.394612957,
            "range": "12560749.10–12610228.20",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5731528.294458482,
            "range": "5715535.60–5746315.53",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12631482.736962266,
            "range": "12616158.40–12650288.25",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 5022392.649521245,
            "range": "5005348.75–5049933.73",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15339071.486615825,
            "range": "15230401.96–15482894.77",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1969583.0681449294,
            "range": "1964626.69–1974551.93",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12835368,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 1713883,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 304025,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 203.48421500000404,
            "range": "174.75–214.42",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 174.74963500001468, 201.37086899997666, 203.48421500000404, 204.2444269999978, 214.41941900004167"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 555.6059060000116,
            "range": "497.90–571.29",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 497.89745599997696, 541.9894179999828, 555.6059060000116, 565.6157990000211, 571.2948370000231"
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
          "id": "65a886efb81e802d94332dbe8c51133f957d1235",
          "message": "fix(editors): use ry-checker on VS Code Marketplace (#338)\n\n* fix(editors): use ry-checker on VS Code Marketplace\n\n* fix(editors): distinguish Marketplace display name\n\n* fix(editors): use a plain hyphen in Marketplace title",
          "timestamp": "2026-09-07T23:53:25+02:00",
          "tree_id": "69308de445097c1f393ecac28387502120eea0a6",
          "url": "https://github.com/sims1253/ry/commit/65a886efb81e802d94332dbe8c51133f957d1235"
        },
        "date": 1788818285982,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1604910.7819983405,
            "range": "1601044.85–1609079.16",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 587992.8699359547,
            "range": "586132.81–590817.93",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5356725.541396654,
            "range": "5338516.04–5380749.24",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 753903.2127421913,
            "range": "752715.07–755317.37",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 9334404.171155008,
            "range": "9303617.05–9368037.73",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1269073.8187647078,
            "range": "1263674.48–1278350.50",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14144437.860162726,
            "range": "14106506.70–14195951.34",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6870623.561005716,
            "range": "6853244.92–6888344.09",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1435506.803498246,
            "range": "1425253.54–1450962.37",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2668618.692882064,
            "range": "2664584.79–2673549.03",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 915147.4652430449,
            "range": "913346.76–917581.72",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6882950.003643719,
            "range": "6868793.05–6900014.84",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1424884.9929354326,
            "range": "1422317.68–1428170.92",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 75684993.85,
            "range": "75457993.51–75921410.06",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12744537.802163782,
            "range": "12690133.07–12809645.94",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5818711.379385712,
            "range": "5800447.46–5836476.18",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12732885.181625752,
            "range": "12657953.06–12823280.54",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4959629.683325356,
            "range": "4921001.42–5007571.73",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15398653.368965233,
            "range": "15269834.75–15567044.86",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1956152.3717642655,
            "range": "1949975.02–1961889.33",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12835368,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 1713883,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 304137,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 207.7988479999476,
            "range": "199.93–234.59",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 199.9341060000006, 203.81813199998578, 207.7988479999476, 217.16901299997699, 234.58699400001206"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 589.1147459999775,
            "range": "556.35–630.66",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 556.3520279999939, 568.7424790000077, 589.1147459999775, 623.6065270000254, 630.6602329999441"
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
          "id": "dde15b6cb94041f5280f588d36013a1b3254dfb3",
          "message": "chore(zed): prepare ry-lsp gallery identity (#339)",
          "timestamp": "2026-09-07T23:53:28+02:00",
          "tree_id": "2f06ce13772b30b8877da9fca699db9c125a3d49",
          "url": "https://github.com/sims1253/ry/commit/dde15b6cb94041f5280f588d36013a1b3254dfb3"
        },
        "date": 1788818579837,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1614468.2026641276,
            "range": "1608752.70–1624332.46",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 586033.3583138088,
            "range": "585050.15–587248.57",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5491877.127230282,
            "range": "5460959.39–5524996.84",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 765940.9569664701,
            "range": "764266.70–767636.83",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 9733755.058624253,
            "range": "9590084.57–9896716.28",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1279015.62340703,
            "range": "1277702.44–1280441.74",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 14201095.86417586,
            "range": "14138659.45–14274340.73",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 7021799.048949001,
            "range": "7006251.03–7039305.60",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1462561.5126259283,
            "range": "1430813.06–1513336.99",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2683463.962594824,
            "range": "2678328.30–2690289.01",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 921910.9486066554,
            "range": "919141.65–925613.63",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 7020861.166638789,
            "range": "7001211.42–7043050.00",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1423264.9410469984,
            "range": "1421814.74–1424927.96",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 73880597.89999999,
            "range": "73526450.02–74247096.21",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 12600115.882711876,
            "range": "12540092.19–12663396.84",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5827599.5932841385,
            "range": "5807550.27–5846706.50",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 12685775.521937454,
            "range": "12634174.25–12742510.63",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4934165.222421867,
            "range": "4886487.69–4995165.48",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 15085156.140728012,
            "range": "14967582.78–15228857.15",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1976965.8132295734,
            "range": "1963791.39–1989125.42",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 12835368,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 1713883,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 304137,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 222.33346300001722,
            "range": "192.90–260.44",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 192.90379100001883, 214.68585899996106, 222.33346300001722, 239.03830499999458, 260.4355579999974"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 588.902100999956,
            "range": "564.30–604.21",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 564.2975429999642, 566.069804000028, 588.902100999956, 591.9683850000147, 604.2112549999729"
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
          "id": "842dbca5502463bb346fb51996ac9f9d12fa0322",
          "message": "Merge pull request #340 from sims1253/perf/startup-and-size\n\nperf: binary size, extension startup, and CLI speed run",
          "timestamp": "2026-09-08T22:17:11+02:00",
          "tree_id": "256176f6164819bf35c1f7aa52c16cc374126710",
          "url": "https://github.com/sims1253/ry/commit/842dbca5502463bb346fb51996ac9f9d12fa0322"
        },
        "date": 1788899077549,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1318149.3583582416,
            "range": "1316509.09–1320002.26",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 462651.47139105183,
            "range": "461831.98–463522.14",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5040929.207848443,
            "range": "5016073.18–5071343.59",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 714985.9578369705,
            "range": "713574.89–716472.23",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 8861497.859232914,
            "range": "8835694.65–8888349.78",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1210480.8789328735,
            "range": "1208826.16–1212308.54",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 11356681.032953924,
            "range": "11313309.62–11395794.47",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6540909.956675054,
            "range": "6527458.20–6555827.21",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1365550.9691407045,
            "range": "1363447.44–1367943.98",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2506182.8278687596,
            "range": "2500548.13–2512700.81",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 857261.0623243476,
            "range": "854320.27–862189.91",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6516944.664475335,
            "range": "6504868.39–6528769.07",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1358793.9739442954,
            "range": "1355685.38–1362962.43",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 75875293.725,
            "range": "75637761.25–76126564.36",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 10588459.797138058,
            "range": "10543000.86–10635239.74",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5817319.709586781,
            "range": "5785984.17–5857468.96",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 10555632.201642314,
            "range": "10505191.69–10609717.83",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4018253.7128447345,
            "range": "4005103.63–4031719.30",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 12748870.354382014,
            "range": "12704082.69–12794631.29",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1888928.0124878916,
            "range": "1878068.36–1903942.57",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 8932328,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 754975,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 213585,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 213.7986530000344,
            "range": "203.09–262.05",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 203.08990699995775, 209.3755129999481, 213.7986530000344, 251.2493540000869, 262.0475490000099"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 606.2168110000202,
            "range": "560.05–651.64",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 560.0493250000291, 565.2549979999894, 606.2168110000202, 618.7206080000615, 651.6399589999346"
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
          "id": "8f1f5f52f5da938b07c9120edf2450d1355b3193",
          "message": "Merge pull request #385 from sims1253/dependabot/github_actions/r-lib/actions/setup-r-dependencies-2.13.0\n\nchore(deps): bump r-lib/actions/setup-r-dependencies from 2.12.1 to 2.13.0",
          "timestamp": "2026-09-08T22:31:22+02:00",
          "tree_id": "d02c0643ed6370e682501841393e4fb79f37322c",
          "url": "https://github.com/sims1253/ry/commit/8f1f5f52f5da938b07c9120edf2450d1355b3193"
        },
        "date": 1788899865735,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1312203.823560803,
            "range": "1309256.83–1316279.12",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 472354.1194937856,
            "range": "471846.62–472921.19",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 4939778.216706352,
            "range": "4934156.71–4946532.66",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 711742.4965468359,
            "range": "709151.14–714930.07",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 8805582.378400994,
            "range": "8791380.45–8820399.37",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1218390.2301718064,
            "range": "1217132.10–1219759.44",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 11211157.888509456,
            "range": "11124141.90–11332325.45",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6623005.313958076,
            "range": "6613521.02–6635079.62",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1378560.5216920576,
            "range": "1368876.18–1394620.61",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2498036.0854715635,
            "range": "2494997.56–2501533.27",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 859998.4464530444,
            "range": "858641.81–861655.16",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6566430.287821826,
            "range": "6557285.81–6575203.56",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1364580.812313677,
            "range": "1360809.62–1370360.51",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 68159580.45,
            "range": "67610910.57–68765729.14",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 10545743.943243507,
            "range": "10508293.15–10586038.01",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5800660.065402388,
            "range": "5762441.14–5853295.23",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 10541182.949019942,
            "range": "10473426.14–10631106.72",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4018939.7308109826,
            "range": "4008484.48–4029420.50",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 12552022.144877266,
            "range": "12521963.11–12586994.33",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1850296.6135457302,
            "range": "1844423.24–1859797.89",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 8932328,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 754975,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 213585,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 227.59131600003457,
            "range": "192.27–230.29",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 192.26646800001618, 217.98511999996845, 227.59131600003457, 230.14217599999392, 230.2897159999702"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 580.4126420000102,
            "range": "536.55–621.49",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 536.5475960000185, 553.4212779999943, 580.4126420000102, 616.1715139999869, 621.4869450000115"
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
          "id": "f9adc674e738fb3cb96a227532d779d728cec749",
          "message": "Merge pull request #387 from sims1253/dependabot/github_actions/r-lib/actions/setup-r-2.13.0\n\nchore(deps): bump r-lib/actions/setup-r from 2.12.1 to 2.13.0",
          "timestamp": "2026-09-08T22:31:30+02:00",
          "tree_id": "18c3ac4d5dc835cac0be176008cb4a3c56beeb7d",
          "url": "https://github.com/sims1253/ry/commit/f9adc674e738fb3cb96a227532d779d728cec749"
        },
        "date": 1788900508873,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 767753.9855738267,
            "range": "766753.89–768828.31",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 261746.08370117797,
            "range": "261432.26–262124.32",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 3476600.2946216306,
            "range": "3469400.69–3484580.15",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 481540.8792991854,
            "range": "480393.20–482821.54",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 6122540.997605193,
            "range": "6115625.17–6130058.25",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 834827.6594820602,
            "range": "832986.10–836814.20",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 6796417.89993736,
            "range": "6734559.41–6848750.69",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 4117985.8853161074,
            "range": "4108115.31–4129033.46",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 841473.9373752499,
            "range": "839078.84–844342.83",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 1355776.7294079126,
            "range": "1352574.19–1359047.50",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 482714.3187722679,
            "range": "482068.55–483487.93",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 4123398.08551267,
            "range": "4115737.57–4131428.25",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 843876.9685363611,
            "range": "842888.16–844897.98",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 51598598.55,
            "range": "51226847.02–51960104.02",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 6592960.379914951,
            "range": "6530816.18–6648701.70",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 3445248.1717367196,
            "range": "3432872.67–3463394.55",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 6805495.121117164,
            "range": "6771905.06–6841158.18",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 2388291.7529152622,
            "range": "2369864.61–2407473.19",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 7606456.548826294,
            "range": "7566431.66–7647000.13",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1464737.942953895,
            "range": "1455713.31–1474045.97",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 8932328,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 754975,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 213585,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 134.14966599998297,
            "range": "107.32–142.91",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 107.3226030000369, 132.29304400004912, 134.14966599998297, 136.21389100002125, 142.91406799998367"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 457.6774700000533,
            "range": "399.48–471.41",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 399.48172400001204, 443.96845600003144, 457.6774700000533, 460.503796999983, 471.4069819999859"
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
          "id": "d6cb358690e304a307b8ff4d8bb83e15b23a6180",
          "message": "Merge pull request #386 from sims1253/dependabot/github_actions/actions/deploy-pages-5.0.1\n\nchore(deps): bump actions/deploy-pages from 4.0.5 to 5.0.1",
          "timestamp": "2026-09-08T23:13:21+02:00",
          "tree_id": "060a6f06d652c84940c03362116f34c65f0e7e06",
          "url": "https://github.com/sims1253/ry/commit/d6cb358690e304a307b8ff4d8bb83e15b23a6180"
        },
        "date": 1788902341359,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1053625.561028302,
            "range": "1051442.90–1056689.39",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 363877.9038450969,
            "range": "361788.28–366488.08",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 3927487.785905049,
            "range": "3918785.58–3937597.80",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 548135.8949827659,
            "range": "547297.76–549116.78",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 6954229.395738885,
            "range": "6907991.44–7013564.09",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 948862.4992315067,
            "range": "946214.99–952004.62",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 8160015.532892535,
            "range": "8134077.86–8188481.25",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 4805430.016559878,
            "range": "4771927.20–4843880.89",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1040919.8125540994,
            "range": "1028580.13–1064208.16",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 1826798.7200218397,
            "range": "1817893.47–1837273.89",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 643423.5284945851,
            "range": "642696.50–644209.35",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 4814622.684606916,
            "range": "4801965.57–4828192.16",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1025027.5605661223,
            "range": "1022954.33–1026969.06",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 62343337.9,
            "range": "61322467.68–63327996.98",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 7774691.712797223,
            "range": "7751461.44–7804026.86",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 4501658.042418564,
            "range": "4478630.46–4536096.01",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 7790273.24489067,
            "range": "7773876.94–7810724.39",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 2991841.589366245,
            "range": "2980526.99–3001805.36",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 9327514.12215851,
            "range": "9308557.64–9354144.26",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1402572.0920472308,
            "range": "1399146.25–1407157.02",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 8932328,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 754975,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 213585,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 160.951790000021,
            "range": "148.18–190.61",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 148.1751130000339, 158.9890510000405, 160.951790000021, 179.43254699994577, 190.60908899997594"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 503.92144800000824,
            "range": "451.32–531.82",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 451.318682000041, 480.10585200000787, 503.92144800000824, 505.8048269999563, 531.8168189999997"
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
          "id": "05c9ba9442dfd233cfc2a2bf5471c7c9ba03f3c5",
          "message": "Merge pull request #388 from sims1253/dependabot/github_actions/actions/upload-pages-artifact-5.0.0\n\nchore(deps): bump actions/upload-pages-artifact from 4.0.0 to 5.0.0",
          "timestamp": "2026-09-08T23:13:26+02:00",
          "tree_id": "3dbf8c6460aae0c60d8d51b9e9409dee9ac71a4e",
          "url": "https://github.com/sims1253/ry/commit/05c9ba9442dfd233cfc2a2bf5471c7c9ba03f3c5"
        },
        "date": 1788902634481,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 936447.6817917662,
            "range": "921389.04–949483.78",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 288081.44190319633,
            "range": "284289.14–293698.85",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 3810108.3713826416,
            "range": "3777447.67–3860326.30",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 497902.25126954465,
            "range": "482732.06–516310.21",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 6340588.503961121,
            "range": "6332192.73–6349967.50",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 855246.3938387195,
            "range": "842191.10–873524.90",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 7336005.806142481,
            "range": "7305553.00–7369922.03",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 4250515.5250333855,
            "range": "4187231.06–4328687.37",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1004162.1951956382,
            "range": "975844.23–1032538.98",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 1465831.6734461882,
            "range": "1446827.50–1494275.21",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 515271.4009311974,
            "range": "514749.84–515844.96",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 4193549.8561318377,
            "range": "4180771.30–4208678.55",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 928230.5347114172,
            "range": "915549.93–944863.65",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 38043967.45999999,
            "range": "37117434.05–39156267.04",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 7452171.545184471,
            "range": "7254004.26–7652913.81",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 3996579.462027979,
            "range": "3974909.50–4016762.48",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 7014435.713610639,
            "range": "6916717.37–7160677.96",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 2708092.8816508413,
            "range": "2673747.98–2749215.12",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 8026213.926541835,
            "range": "7965181.30–8113555.26",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1526484.0196624633,
            "range": "1510660.40–1549265.92",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 8932328,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 754975,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 213585,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 123.59607400000095,
            "range": "122.13–144.49",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 122.12952000001678, 122.8935829999973, 123.59607400000095, 142.10299100005068, 144.49266000004718"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 518.719429000048,
            "range": "420.08–645.38",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 420.0803430000087, 425.8384579999838, 518.719429000048, 576.1972480000113, 645.378337000031"
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
          "id": "90c88a4d93d18d1cb0fad79882edb67f87c188f3",
          "message": "Merge pull request #393 from sims1253/fix/serialized-inventory-default-cap\n\nfix(config): raise default serialized-data byte cap to 16 MiB",
          "timestamp": "2026-09-08T23:54:13+02:00",
          "tree_id": "40bc9187d1ba5e073a73b48b967656fa9bee912a",
          "url": "https://github.com/sims1253/ry/commit/90c88a4d93d18d1cb0fad79882edb67f87c188f3"
        },
        "date": 1788904791173,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1296916.2799503647,
            "range": "1295689.21–1298301.18",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 473587.9211888305,
            "range": "473084.48–474189.48",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 4979242.970724849,
            "range": "4974808.56–4983352.52",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 708329.1405439142,
            "range": "707810.08–708847.28",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 8854990.116029473,
            "range": "8826304.51–8904641.97",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1218650.9071155414,
            "range": "1216395.34–1221358.71",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 11369561.93455619,
            "range": "11205931.17–11609657.92",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6572603.542491284,
            "range": "6543992.80–6622164.37",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1373680.7293304198,
            "range": "1372330.45–1375175.42",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2485390.980672217,
            "range": "2483460.82–2487470.27",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 859585.345363838,
            "range": "858615.18–860716.18",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6571048.767059354,
            "range": "6550798.32–6600243.94",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1367586.4422598882,
            "range": "1365695.59–1370083.48",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 63266844.26666667,
            "range": "62977133.88–63571387.12",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 10537366.250019955,
            "range": "10496297.62–10582524.68",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5760851.12637695,
            "range": "5737929.64–5783680.43",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 10632257.6736509,
            "range": "10562091.20–10723749.74",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 3973135.822805156,
            "range": "3967441.06–3979253.68",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 12532269.046047512,
            "range": "12519087.65–12546982.50",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1889589.8576822323,
            "range": "1877456.13–1901599.94",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 8932328,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 754975,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 213585,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 219.7144289999851,
            "range": "187.67–239.87",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 187.67044300003909, 192.50013699999545, 219.7144289999851, 223.35303300002124, 239.8719160000328"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 576.3693070000154,
            "range": "559.81–595.69",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 559.8144230000325, 568.1767040000414, 576.3693070000154, 586.9498240000103, 595.6937840000028"
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
          "id": "05ed92159c816e9831bfef8003211befca060bcf",
          "message": "Merge pull request #394 from sims1253/fix/data-mask-pronoun-opaque\n\nfix(checker): keep the data-mask pronoun opaque without a data argument",
          "timestamp": "2026-09-09T11:18:45+02:00",
          "tree_id": "c28ddd139cbfd7ff96070648783edbb76438be77",
          "url": "https://github.com/sims1253/ry/commit/05ed92159c816e9831bfef8003211befca060bcf"
        },
        "date": 1788945850379,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1253870.5769471605,
            "range": "1250966.97–1258991.67",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 425268.1415425235,
            "range": "424506.55–426389.47",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 4940830.407723005,
            "range": "4930825.91–4952599.53",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 661189.4443666937,
            "range": "660134.38–662783.90",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 8460491.362177987,
            "range": "8444918.03–8477635.20",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1147375.2047859842,
            "range": "1145222.68–1150109.15",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 9955313.424495477,
            "range": "9905543.20–10030994.61",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 5599122.051272454,
            "range": "5592825.33–5607283.52",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1254958.6928722113,
            "range": "1242243.95–1273718.85",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2089931.50978106,
            "range": "2085282.35–2097408.90",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 767798.4218227481,
            "range": "765906.96–769950.37",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 5627503.8968031965,
            "range": "5620190.35–5636505.81",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1249016.558320623,
            "range": "1246965.26–1251691.64",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 68463118.73333333,
            "range": "68106859.12–68911738.62",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 9644081.079508217,
            "range": "9609933.94–9681750.15",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5446963.187359532,
            "range": "5429531.05–5466623.46",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 9590963.230490796,
            "range": "9543010.20–9651354.71",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 3707573.9669604963,
            "range": "3683163.58–3737055.00",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 11519311.732900426,
            "range": "11476706.58–11560201.44",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 2117341.722916033,
            "range": "2112350.20–2122724.14",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 8932744,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 754975,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 213585,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 189.613141000038,
            "range": "167.41–243.70",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 167.40544500004034, 172.36492299998645, 189.613141000038, 202.20203200000105, 243.7046960000298"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 542.3529470000067,
            "range": "528.46–734.70",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 528.4609390000114, 535.684709000052, 542.3529470000067, 563.5015600000042, 734.6961110000266"
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
          "id": "da8417cbd544aff9895cecc776c16193828627f1",
          "message": "Merge pull request #399 from sims1253/chore/sync-mirai-status-contract\n\nchore(typeshed): sync mirai status contract",
          "timestamp": "2026-09-09T11:18:50+02:00",
          "tree_id": "012cfcdcf8009ef630f3784fd210e3bee75c887d",
          "url": "https://github.com/sims1253/ry/commit/da8417cbd544aff9895cecc776c16193828627f1"
        },
        "date": 1788946179195,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1308566.9213445864,
            "range": "1307192.32–1310055.41",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 483532.72325006063,
            "range": "482080.24–485519.56",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 4939763.954702375,
            "range": "4932289.41–4950514.58",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 701861.299568202,
            "range": "699455.31–705465.91",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 8759674.839921983,
            "range": "8747033.20–8773759.92",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1212052.9159417474,
            "range": "1210700.66–1213374.56",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 11207386.171460597,
            "range": "11137216.87–11298300.25",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6490075.29565103,
            "range": "6471136.48–6512028.65",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1365732.3239502788,
            "range": "1350973.64–1388074.51",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2314938.0517291324,
            "range": "2312601.85–2317504.33",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 845532.8026613634,
            "range": "836367.58–862905.23",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6513733.504395381,
            "range": "6459742.21–6604091.53",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1354297.2035025563,
            "range": "1350709.69–1358875.73",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 73211036.01666667,
            "range": "72813462.29–73639150.98",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 10556341.443629337,
            "range": "10521620.64–10595620.16",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5739375.223910608,
            "range": "5705585.12–5783819.42",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 10643756.02510731,
            "range": "10602256.56–10692949.81",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 3994015.2498949384,
            "range": "3971791.33–4022015.02",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 12464385.236061037,
            "range": "12441825.21–12490733.86",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1900241.4648920284,
            "range": "1891263.99–1908222.19",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 8932872,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 754975,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 213585,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 212.89979600004153,
            "range": "199.28–260.26",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 199.2788189999992, 205.89183099998627, 212.89979600004153, 232.62572300003376, 260.2644079999882"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 578.4200520000304,
            "range": "515.57–660.72",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 515.5675879999762, 540.981116000039, 578.4200520000304, 597.616578999965, 660.7238609999768"
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
          "id": "56fe5fedeb3f53b5b2617dd18dd61ac03ed8e1d6",
          "message": "Merge pull request #389 from sims1253/dependabot/github_actions/zizmorcore/zizmor-action-0.6.3\n\nchore(deps): bump zizmorcore/zizmor-action from 0.6.2 to 0.6.3",
          "timestamp": "2026-09-09T11:29:56+02:00",
          "tree_id": "01c896e8565f949d4d52b5b41cc13b88bf02e643",
          "url": "https://github.com/sims1253/ry/commit/56fe5fedeb3f53b5b2617dd18dd61ac03ed8e1d6"
        },
        "date": 1788946537906,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1308245.7973096403,
            "range": "1305800.67–1311342.97",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 478722.68286037137,
            "range": "476366.13–482041.65",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 4976082.27144462,
            "range": "4967795.37–4985219.57",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 707466.2203235388,
            "range": "704744.85–712006.44",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 8751140.96903749,
            "range": "8728342.49–8779264.14",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1203743.7896299625,
            "range": "1201898.30–1206637.01",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 11175350.919652581,
            "range": "11143685.31–11207925.71",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6482662.467650611,
            "range": "6472562.65–6494879.63",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1380593.368482933,
            "range": "1358801.66–1406824.49",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2320197.2204629593,
            "range": "2317126.56–2323297.20",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 843730.5968496377,
            "range": "841382.61–846958.86",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6476615.916699489,
            "range": "6466429.39–6487319.32",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1352769.2514156653,
            "range": "1351346.15–1354839.22",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 69970576.85000002,
            "range": "69304050.11–70643981.83",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 10514678.05597759,
            "range": "10470071.21–10559694.64",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5824721.292350141,
            "range": "5808632.77–5841357.63",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 10557543.3705871,
            "range": "10518269.44–10600797.46",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4480363.6270551095,
            "range": "4330923.70–4636864.42",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 12919608.815895269,
            "range": "12714522.08–13153578.40",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1875221.5527969566,
            "range": "1865313.72–1888777.40",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 8932872,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 754975,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 213585,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 230.44258800003445,
            "range": "199.67–263.70",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 199.6692729999777, 203.3034869999974, 230.44258800003445, 247.73488300002646, 263.6963869999745"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 569.3386670000036,
            "range": "542.84–650.61",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 542.8402260000003, 550.1797950000037, 569.3386670000036, 622.5119290000293, 650.6084810000029"
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
          "id": "a4e5a6d882ceb22889f0b87d3f5bf140a1a17032",
          "message": "Merge pull request #395 from sims1253/fix/341-opaque-classless-stub-return\n\nfix(checker): opaque stub returns without class entries read as class-unknown",
          "timestamp": "2026-09-09T14:12:54+02:00",
          "tree_id": "667ef45bcb4d2e073c57d12655c1ee9d8cb00914",
          "url": "https://github.com/sims1253/ry/commit/a4e5a6d882ceb22889f0b87d3f5bf140a1a17032"
        },
        "date": 1788956296655,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 870577.8345910665,
            "range": "865692.00–876544.10",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 301974.9470008234,
            "range": "297502.91–308692.37",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 3760590.5844629323,
            "range": "3747935.74–3774070.80",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 486756.84798722004,
            "range": "484903.71–488847.38",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 6461559.5717695,
            "range": "6360069.74–6599898.88",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 850374.3612538539,
            "range": "847126.13–853730.37",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 7638732.01416371,
            "range": "7566662.84–7718650.36",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 4280386.147389796,
            "range": "4192872.94–4389573.27",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 935403.9734075342,
            "range": "922930.70–956026.20",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 1583334.8401981709,
            "range": "1536834.15–1637912.39",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 524017.0354875446,
            "range": "523104.80–524899.34",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 4282011.869789941,
            "range": "4223497.62–4360707.81",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 945389.5031006869,
            "range": "935984.78–960569.03",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 46181526.125,
            "range": "44692000.70–47986324.29",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 7021352.322538058,
            "range": "6963358.31–7099983.56",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 4107324.361229873,
            "range": "4026113.41–4216986.54",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 7136793.837984895,
            "range": "7063532.71–7229967.56",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 2814450.5410390114,
            "range": "2781244.23–2851135.23",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 8292809.0052992385,
            "range": "8156740.34–8489562.39",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1581544.9485392363,
            "range": "1534425.46–1643210.00",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 8933224,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 754975,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 213585,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 144.8937520000036,
            "range": "120.70–395.13",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 120.70035599998664, 131.94753099995432, 144.8937520000036, 184.09238300001016, 395.13025000004563"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 509.08170099998824,
            "range": "399.67–688.54",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 399.6722149999696, 453.3093289999524, 509.08170099998824, 596.6307329999981, 688.5439500000211"
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
          "id": "960814deaee6f1fe0dacf55633bc2aba195b300e",
          "message": "Merge pull request #396 from sims1253/pub/function-mode-call-heads\n\nfix(checker): function-mode call-head lookup for bare symbols",
          "timestamp": "2026-09-09T14:59:30+02:00",
          "tree_id": "a4e64fba89f20b20ad61c947a2c7fa5e70bce24f",
          "url": "https://github.com/sims1253/ry/commit/960814deaee6f1fe0dacf55633bc2aba195b300e"
        },
        "date": 1788959117322,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1328623.1631888268,
            "range": "1325050.85–1333177.27",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 480092.3914562105,
            "range": "476084.45–485474.21",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 4992771.496825586,
            "range": "4968177.47–5029652.72",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 706011.2978213817,
            "range": "704886.35–707525.80",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 8799283.15853243,
            "range": "8760042.68–8866979.06",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1222694.5928862125,
            "range": "1221398.98–1224067.37",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 11226790.420736285,
            "range": "11086380.65–11398032.06",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6473562.67086359,
            "range": "6429005.17–6535526.01",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1354273.8872904715,
            "range": "1352911.37–1355699.15",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2389904.852962919,
            "range": "2387677.04–2392301.48",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 840974.8652731491,
            "range": "839382.79–842794.19",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6455539.792392334,
            "range": "6433336.12–6482321.34",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1351618.9263668656,
            "range": "1350199.08–1353096.78",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 72165226.4,
            "range": "71646308.31–72680888.47",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 10451008.045806393,
            "range": "10422749.74–10482025.59",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5778404.22236921,
            "range": "5725507.98–5851264.22",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 10469933.253767226,
            "range": "10429333.12–10517779.21",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 3962742.2438798533,
            "range": "3946669.63–3981423.23",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 12530286.068211433,
            "range": "12463369.44–12629415.76",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1869551.119510983,
            "range": "1863437.06–1877728.10",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 8933736,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 754975,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 213585,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 228.52503200003412,
            "range": "204.61–266.51",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 204.61267900001258, 212.0488919999916, 228.52503200003412, 236.92882599995937, 266.5111119999783"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 599.937498000043,
            "range": "556.05–635.12",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 556.0511360000237, 561.8808959999587, 599.937498000043, 629.6818280000007, 635.1154119999846"
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
          "id": "5bb8caa797ee980793924050cb9207a0ee3ca9d8",
          "message": "Merge pull request #400 from sims1253/fix/342-root-anchored-invalidation\n\nfix(checker): bound forwarded-default invalidation to root calls",
          "timestamp": "2026-09-09T15:54:34+02:00",
          "tree_id": "c14b487468cf48b0bb6f9db227bfdcf7ccd104d0",
          "url": "https://github.com/sims1253/ry/commit/5bb8caa797ee980793924050cb9207a0ee3ca9d8"
        },
        "date": 1788962412598,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1362279.0293169522,
            "range": "1359842.48–1365363.31",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 475371.64469087095,
            "range": "474669.90–476373.99",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 4978962.083730981,
            "range": "4969723.45–4989169.35",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 703513.5855402055,
            "range": "702613.48–704452.04",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 8895823.666661767,
            "range": "8850378.93–8942160.00",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1205986.961362148,
            "range": "1202202.95–1209862.38",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 11317952.521402229,
            "range": "11271636.88–11377045.66",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6112387.189516459,
            "range": "6103754.43–6120497.95",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1346503.4513628006,
            "range": "1333760.20–1362202.20",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2366569.150057473,
            "range": "2358818.86–2378543.71",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 838468.5561415505,
            "range": "837096.68–840158.47",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6130156.632662312,
            "range": "6120303.12–6141511.78",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1330108.6238221773,
            "range": "1327410.28–1333044.05",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 78662595.575,
            "range": "77828185.84–79497657.46",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 10751929.846908385,
            "range": "10708834.01–10814299.73",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5874913.613067179,
            "range": "5844972.71–5902261.41",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 10750273.397321854,
            "range": "10696428.33–10810314.19",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4070383.8558626333,
            "range": "4054198.06–4087560.00",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 12868345.94515582,
            "range": "12809601.93–12943772.88",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1861704.874387319,
            "range": "1856089.72–1868857.04",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 8936360,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 754975,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 213585,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 204.18651199992746,
            "range": "194.18–276.67",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 194.17544999998063, 194.5192869999446, 204.18651199992746, 205.75784599990584, 276.6655360000441"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 555.5225749999518,
            "range": "545.20–649.66",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 545.1993329999968, 551.0540659999242, 555.5225749999518, 555.659389999928, 649.662207000074"
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
          "id": "5800678e5c8c07cda5bd87aa7f7c838a81ce6c9c",
          "message": "Merge pull request #397 from sims1253/fix/343-default-parameter-else-branch\n\nfix(checker): a rejected mode test degrades default-parameter types",
          "timestamp": "2026-09-09T17:48:49+02:00",
          "tree_id": "9a5290e0716edf139f0412dc476d18ce198f39d7",
          "url": "https://github.com/sims1253/ry/commit/5800678e5c8c07cda5bd87aa7f7c838a81ce6c9c"
        },
        "date": 1788969284521,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1324781.3951350998,
            "range": "1321423.01–1328645.74",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 465274.0433445424,
            "range": "464337.50–466300.82",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5022921.879837347,
            "range": "5003041.10–5042616.15",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 701844.9799703413,
            "range": "698954.10–705847.49",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 8824339.838687763,
            "range": "8806084.57–8845264.22",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1206814.3687403097,
            "range": "1202563.08–1213149.56",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 11208998.214474682,
            "range": "11153409.98–11262774.71",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 6530899.154295829,
            "range": "6519031.94–6543328.24",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1369915.1124441205,
            "range": "1363793.79–1378969.54",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2503635.718388702,
            "range": "2500456.56–2507106.46",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 858115.4630877739,
            "range": "856291.94–860563.19",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 6516746.535442312,
            "range": "6501687.18–6533488.21",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1361473.8984288266,
            "range": "1358787.34–1364347.49",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 73146534.35,
            "range": "72408774.70–73857264.47",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 10572327.427299853,
            "range": "10512132.51–10640058.77",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5726657.832393328,
            "range": "5712861.31–5741166.99",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 10494304.059468675,
            "range": "10469665.94–10522125.17",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 4057537.587336114,
            "range": "4039823.62–4077703.95",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 12553964.186031152,
            "range": "12513869.43–12594539.38",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 1836273.6666714803,
            "range": "1830424.27–1843646.09",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 8936936,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 754975,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 213585,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 204.3370780000114,
            "range": "195.43–261.19",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 195.43338299996685, 199.848805000016, 204.3370780000114, 220.35201999999117, 261.18552300002193"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 556.2726600000169,
            "range": "552.58–592.75",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 552.5797439999878, 556.0793709999998, 556.2726600000169, 579.7287399999914, 592.7475369999884"
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
          "id": "4a6ed02b57634053bad94adbdbcc6056aeff9285",
          "message": "Merge pull request #398 from sims1253/fix/373-condition-coercion\n\nfix(conditions): accept R-coercible scalar if/while conditions",
          "timestamp": "2026-09-09T18:59:15+02:00",
          "tree_id": "2b1df1e953094c080d211bc36cac91678e8597bc",
          "url": "https://github.com/sims1253/ry/commit/4a6ed02b57634053bad94adbdbcc6056aeff9285"
        },
        "date": 1788973544935,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "core/check_branch_scopes/1024",
            "value": 1269252.4143993629,
            "range": "1267113.04–1272129.00",
            "unit": "ns"
          },
          {
            "name": "core/check_branch_scopes/128",
            "value": 423855.8054567597,
            "range": "423378.30–424404.93",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/1024",
            "value": 5130576.1048914995,
            "range": "5113090.81–5150118.60",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/one_arm/128",
            "value": 674229.1448028637,
            "range": "669956.64–681496.23",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/1024",
            "value": 8850675.969194008,
            "range": "8762374.94–8959364.50",
            "unit": "ns"
          },
          {
            "name": "core/check_if_expression_scopes/two_arms/128",
            "value": 1170665.1878529661,
            "range": "1167986.59–1173848.68",
            "unit": "ns"
          },
          {
            "name": "core/check_project_glue",
            "value": 10521210.266351575,
            "range": "10264550.56–10857338.33",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/1024",
            "value": 5729468.4835262755,
            "range": "5718875.07–5741475.32",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/and/128",
            "value": 1282847.6381530077,
            "range": "1277666.04–1287896.93",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/1024",
            "value": 2130574.299879306,
            "range": "2128109.17–2133418.90",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/assert/128",
            "value": 777219.4770903637,
            "range": "772040.19–783731.71",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/1024",
            "value": 5758982.091127863,
            "range": "5743232.63–5778085.34",
            "unit": "ns"
          },
          {
            "name": "core/check_selected_branch_scopes/or/128",
            "value": 1265492.8206609106,
            "range": "1262672.52–1268492.73",
            "unit": "ns"
          },
          {
            "name": "core/check_single_synthetic",
            "value": 70455832.76666668,
            "range": "70230994.98–70666637.63",
            "unit": "ns"
          },
          {
            "name": "core/lsp_edit_sim",
            "value": 9848026.28758812,
            "range": "9782361.69–9921273.79",
            "unit": "ns"
          },
          {
            "name": "core/parse_large",
            "value": 5471257.704610174,
            "range": "5448770.04–5494639.20",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_dependent",
            "value": 9836855.540168602,
            "range": "9786182.90–9886350.05",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_leaf",
            "value": 3800014.634198046,
            "range": "3762916.00–3839196.58",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_library",
            "value": 11853251.912595535,
            "range": "11786155.93–11924530.97",
            "unit": "ns"
          },
          {
            "name": "core/warm_edit_sparse_callers",
            "value": 2189018.854228585,
            "range": "2170199.15–2215008.95",
            "unit": "ns"
          },
          {
            "name": "cli/executable",
            "value": 8937416,
            "unit": "bytes"
          },
          {
            "name": "vscode/javascript",
            "value": 754975,
            "unit": "bytes"
          },
          {
            "name": "vscode/vsix-without-server",
            "value": 213585,
            "unit": "bytes"
          },
          {
            "name": "zed/wasm",
            "value": 394760,
            "unit": "bytes"
          },
          {
            "name": "vscode/activation",
            "value": 201.3622139999643,
            "range": "181.22–235.31",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 181.21977100003278, 190.42315899999812, 201.3622139999643, 224.29971500003012, 235.30638800002635"
          },
          {
            "name": "vscode/activation-to-first-diagnostic",
            "value": 548.9962769999984,
            "range": "524.07–615.48",
            "unit": "ms",
            "extra": "VS Code 1.90.2; median of 5 fresh hosts; samples: 524.0659289999749, 539.8656400000327, 548.9962769999984, 583.9984009999898, 615.4770190000418"
          }
        ]
      }
    ]
  }
}