window.BENCHMARK_DATA = {
  "lastUpdate": 1788738043571,
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
      }
    ]
  }
}