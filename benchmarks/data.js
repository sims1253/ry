window.BENCHMARK_DATA = {
  "lastUpdate": 1788736452798,
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
      }
    ]
  }
}