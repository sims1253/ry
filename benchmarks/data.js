window.BENCHMARK_DATA = {
  "lastUpdate": 1788752832431,
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
      }
    ]
  }
}