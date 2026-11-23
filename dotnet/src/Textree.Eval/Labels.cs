using System.Net;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Gil.Review;
using Textree.Ingest;

namespace Textree.Eval;

/// <summary>
/// A labelling session for a person: the same masked notes the leave-one-out arms saw, each to be
/// filed into one of the tree's folders (the ceiling), and the names the harness made, each to be
/// judged good or not. Written as one self-contained HTML file that stays on this machine; the
/// answers come back as a JSON file the person saves, read by <see cref="Score"/>.
/// </summary>
public static class Labels
{
    public static void Export(string root, LeaveOneOut.Settings settings, JsonObject? harnessReport, JsonObject? casesReport, string outHtml)
    {
        var cases = LeaveOneOut.MaskedCases(root, settings);
        var hierarchy = Hierarchy.Read(new Folder(root));
        var folders = new List<string> { "" };
        folders.AddRange(hierarchy.Documents.SelectMany(d => Ancestors(d.Folder)).Distinct().Order(StringComparer.Ordinal));
        var names = new JsonArray();
        foreach (var row in harnessReport?["arms"]?["harness"]?.AsArray() ?? [])
        {
            var form = row!["form"]?.GetValue<string>();
            if (form is "add_document" or "add_folder" && row["target"] is { } t && (row["run"]?.GetValue<int>() ?? 1) == 1)
            {
                var c = cases.FirstOrDefault(x => x.Path == row["doc"]!.GetValue<string>());
                if (c is not null)
                {
                    names.Add(new JsonObject { ["case"] = c.Id, ["name"] = t.GetValue<string>().Normalize(NormalizationForm.FormC) });
                }
            }
        }
        var edits = new JsonArray();
        foreach (var row in casesReport?["cases"]?.AsArray() ?? [])
        {
            if ((row!["run"]?.GetValue<int>() ?? 1) != 1 || row["edited"] is not JsonArray edited || row["input"] is not { } input)
            {
                continue;
            }
            foreach (var doc in edited)
            {
                var id = $"e{edits.Count + 1:00}";
                edits.Add(new JsonObject
                {
                    ["id"] = id,
                    ["case"] = row["id"]!.GetValue<string>(),
                    ["path"] = doc!["path"]!.GetValue<string>(),
                    ["after"] = doc["after"]?.GetValue<string>(),
                    ["claims"] = new JsonArray(Claims(input.GetValue<string>()).Select((c, i) => (JsonNode)new JsonObject { ["id"] = $"{id}-{i + 1}", ["text"] = c }).ToArray()),
                });
            }
        }
        var data = new JsonObject
        {
            ["folders"] = new JsonArray(folders.Select(f => (JsonNode)(f.Length == 0 ? "(top)" : f.Normalize(NormalizationForm.FormC))).ToArray()),
            ["cases"] = new JsonArray(cases.Select(c => (JsonNode)new JsonObject { ["id"] = c.Id, ["text"] = c.Text }).ToArray()),
            ["names"] = names,
            ["edits"] = edits,
        };
        var json = data.ToJsonString(new JsonSerializerOptions { Encoder = System.Text.Encodings.Web.JavaScriptEncoder.Default });
        File.WriteAllText(outHtml, Page.Replace("__DATA__", json));
    }

    /// <summary>Scores a person's answers: their hit rate against the original places, and how many harness names they judged good.</summary>
    public static JsonObject Score(string root, LeaveOneOut.Settings settings, string answersFile, string labeller)
    {
        var cases = LeaveOneOut.MaskedCases(root, settings);
        var answers = JsonNode.Parse(File.ReadAllText(answersFile))!.AsObject();
        var rows = new JsonArray();
        var hits = 0;
        var n = 0;
        foreach (var c in cases)
        {
            var chosen = answers["placement"]?[c.Id]?.GetValue<string>();
            if (chosen is null)
            {
                continue;
            }
            var landing = chosen == "(top)" ? "" : chosen;
            var row = LeaveOneOut.Row(c.Path, c.Folder, landing, null);
            row["case"] = c.Id;
            rows.Add(row);
            n++;
            hits += row["hit"]!.GetValue<bool>() ? 1 : 0;
        }
        var claims = answers["edits"]?.AsObject() ?? [];
        var carried = claims.Count(kv => kv.Value?.GetValue<string>() == "kept");
        var good = answers["names"]?.AsObject().Count(kv => kv.Value?.GetValue<string>() == "good") ?? 0;
        var judged = answers["names"]?.AsObject().Count ?? 0;
        return new JsonObject
        {
            ["kind"] = "labels",
            ["labeller"] = labeller,
            ["summary"] = new JsonObject
            {
                [labeller] = new JsonObject
                {
                    ["hit_at_1"] = Json.Of(Proportion.Of(hits, n)),
                    ["partial"] = Json.Of(Mean.Of(rows.Select(r => r!["partial"]!.GetValue<double>()).ToList())),
                },
            },
            ["name_quality"] = Json.Of(Proportion.Of(good, judged)),
            ["section_edit_claim_recall"] = Json.Of(Proportion.Of(carried, claims.Count)),
            ["arms"] = new JsonObject { [labeller] = rows },
        };
    }

    /// <summary>Agreement between two labellers over the cases both filed (Cohen's κ and Gwet's AC1 on "same folder as the original" as two outcomes, and on the folder itself).</summary>
    public static JsonObject Agreement(JsonObject a, string nameA, JsonObject b, string nameB)
    {
        Dictionary<string, JsonObject> RowsOf(JsonObject report, string name) =>
            report["arms"]![name]!.AsArray().Select(r => r!.AsObject()).ToDictionary(r => r["doc"]!.GetValue<string>(), r => r);
        var ra = RowsOf(a, nameA);
        var rb = RowsOf(b, nameB);
        var shared = ra.Keys.Intersect(rb.Keys).Order(StringComparer.Ordinal).ToList();
        var folders = shared.SelectMany(k => new[] { ra[k]["landing"]?.GetValue<string>() ?? "?", rb[k]["landing"]?.GetValue<string>() ?? "?" })
            .Distinct().Order(StringComparer.Ordinal).ToList();
        var hitVerdicts = new List<Verdict>();
        var folderVerdicts = new List<Verdict>();
        foreach (var k in shared)
        {
            hitVerdicts.Add(new Verdict(k, "hit", ra[k]["hit"]!.GetValue<bool>() ? 1 : 0, nameA));
            hitVerdicts.Add(new Verdict(k, "hit", rb[k]["hit"]!.GetValue<bool>() ? 1 : 0, nameB));
            folderVerdicts.Add(new Verdict(k, "folder", folders.IndexOf(ra[k]["landing"]?.GetValue<string>() ?? "?"), nameA));
            folderVerdicts.Add(new Verdict(k, "folder", folders.IndexOf(rb[k]["landing"]?.GetValue<string>() ?? "?"), nameB));
        }
        JsonObject Of(Gil.Review.Agreement g) => new()
        {
            ["pairs"] = g.ComparedPairs,
            ["observed"] = Math.Round(g.Observed, 4),
            ["kappa"] = Json.Num(Math.Round(g.Kappa, 4)),
            ["ac1"] = Math.Round(g.Ac1, 4),
        };
        return new JsonObject
        {
            ["a"] = nameA,
            ["b"] = nameB,
            ["hit"] = shared.Count == 0 ? null : Of(ReviewAgreement.Compute(hitVerdicts, 2)),
            ["folder"] = shared.Count == 0 || folders.Count < 2 ? null : Of(ReviewAgreement.Compute(folderVerdicts, folders.Count)),
        };
    }

    /// <summary>
    /// The claims of a written note, one per sentence or list line, headings and tag-only lines left out — the units a
    /// person marks as carried into the edited document or not.
    /// </summary>
    public static List<string> Claims(string text) =>
        text.Replace("\r\n", "\n").Split('\n')
            .Select(l => l.Trim())
            .Where(l => l.Length > 0 && !l.StartsWith('#') && !l.StartsWith("---", StringComparison.Ordinal))
            .Select(l => l.TrimStart('-', '*', ' '))
            .SelectMany(l => System.Text.RegularExpressions.Regex.Split(l, @"(?<=[.!?])\s+"))
            .Select(c => c.Trim())
            .Where(c => c.Length > 0 && !c.Split(' ', StringSplitOptions.RemoveEmptyEntries).All(w => w.StartsWith('#')))
            .ToList();

    private static IEnumerable<string> Ancestors(string folder)
    {
        for (var f = folder; f.Length > 0; f = Hierarchy.DirOf(f))
        {
            yield return f;
        }
    }

    private const string Page = """
<!doctype html>
<html lang="ko"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Ingest labels</title>
<style>
:root{--bg:#fbfaf7;--fg:#1d1d1b;--muted:#6b6862;--line:#e3dfd6;--accent:#2f5d50;--card:#fff}
@media (prefers-color-scheme:dark){:root{--bg:#171715;--fg:#ecebe6;--muted:#a19e96;--line:#34322d;--accent:#8cc4b1;--card:#201f1c}}
body{background:var(--bg);color:var(--fg);font:15px/1.55 system-ui,-apple-system,"Apple SD Gothic Neo",sans-serif;margin:0;padding:24px 16px 120px}
main{max-width:860px;margin:0 auto}
h1{font-size:20px;margin:0 0 4px}p.lead{color:var(--muted);margin:0 0 20px}
.case{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:14px 16px;margin:0 0 14px}
.case h2{font-size:13px;color:var(--muted);margin:0 0 8px;font-weight:600}
pre{white-space:pre-wrap;word-break:break-word;font:13px/1.5 ui-monospace,Menlo,monospace;max-height:220px;overflow:auto;margin:0 0 10px;background:transparent}
select{width:100%;font:inherit;padding:6px;border-radius:6px;border:1px solid var(--line);background:var(--bg);color:var(--fg)}
.done{border-color:var(--accent)}
.bar{position:fixed;left:0;right:0;bottom:0;background:var(--card);border-top:1px solid var(--line);padding:12px 16px;display:flex;gap:12px;align-items:center;justify-content:center}
button{font:inherit;padding:8px 16px;border-radius:8px;border:0;background:var(--accent);color:var(--bg);cursor:pointer}
label{margin-right:16px}
</style></head><body><main>
<h1>Ingest 라벨 — 이 노트는 어느 폴더에 있었을까</h1>
<p class="lead">각 노트를 원래 있었을 폴더로 고르세요. 링크 같은 위치 단서는 지워져 있습니다. 폴더를 모두 고르면 아래에 하네스가 새로 지은 이름이 열립니다 — 이름은 하네스가 고른 자리를 드러내므로 그 전에는 숨깁니다. 끝나면 «답 저장»으로 JSON 파일을 내려받아 주세요. 고른 값은 이 브라우저에 임시 저장됩니다.</p>
<div id="cases"></div><div id="names-part" hidden><h1 style="margin-top:28px">새로 지은 이름</h1><p class="lead">이 노트에 붙이기 좋은 이름(경로)인가요? 노트 앞부분을 함께 보여 줍니다.</p><div id="names"></div><div id="edits-part"><h1 style="margin-top:28px">절 수정 — 새 글의 주장이 남았나</h1><p class="lead">하네스가 기존 문서의 절을 고쳐 새 글을 넣었습니다. 새 글의 문장마다, 고친 문서에 그 내용이 담겼는지 골라 주세요(같은 말이 아니어도 뜻이 남았으면 «담겼다»).</p><div id="edits"></div></div></div>
</main><div class="bar"><span id="count"></span><button id="save">답 저장</button></div>
<script>
const DATA = __DATA__;
const KEY = "ingest-labels";
let state = {placement:{}, names:{}, edits:{}};
try { state = Object.assign(state, JSON.parse(localStorage.getItem(KEY)) || {}); } catch (e) {}
const keep = () => { try { localStorage.setItem(KEY, JSON.stringify(state)); } catch (e) {} count(); };
const reveal = () => { if (Object.keys(state.placement).length === DATA.cases.length) document.getElementById("names-part").hidden = false; };
const count = () => { reveal(); document.getElementById("count").textContent = `폴더 ${Object.keys(state.placement).length}/${DATA.cases.length} · 이름 ${Object.keys(state.names).length}/${DATA.names.length} · 절 ${Object.keys(state.edits).length}/${DATA.edits.reduce((n, e) => n + e.claims.length, 0)}`; };
const cases = document.getElementById("cases");
DATA.cases.forEach((c, i) => {
  const box = document.createElement("section"); box.className = "case";
  const h = document.createElement("h2"); h.textContent = `${i + 1}. ${c.id}`; box.append(h);
  const pre = document.createElement("pre"); pre.textContent = c.text; box.append(pre);
  const sel = document.createElement("select");
  const none = document.createElement("option"); none.value = ""; none.textContent = "— 폴더를 고르세요 —"; sel.append(none);
  DATA.folders.forEach(f => { const o = document.createElement("option"); o.value = f; o.textContent = f; sel.append(o); });
  sel.value = state.placement[c.id] || "";
  if (sel.value) box.classList.add("done");
  sel.onchange = () => { if (sel.value) { state.placement[c.id] = sel.value; box.classList.add("done"); } else { delete state.placement[c.id]; box.classList.remove("done"); } keep(); };
  box.append(sel); cases.append(box);
});
const names = document.getElementById("names");
DATA.names.forEach(n => {
  const box = document.createElement("section"); box.className = "case";
  const no = DATA.cases.findIndex(c => c.id === n.case);
  const h = document.createElement("h2"); h.textContent = `${no + 1}. ${n.case}`; box.append(h);
  const ex = document.createElement("pre"); ex.textContent = no < 0 ? "" : DATA.cases[no].text.slice(0, 600); box.append(ex);
  const p = document.createElement("pre"); p.style.fontWeight = "600"; p.textContent = "→ " + n.name; box.append(p);
  ["good", "bad"].forEach(v => {
    const l = document.createElement("label"); const r = document.createElement("input");
    r.type = "radio"; r.name = "n-" + n.case; r.value = v; r.checked = state.names[n.case] === v;
    r.onchange = () => { state.names[n.case] = v; keep(); };
    l.append(r, document.createTextNode(v === "good" ? " 좋다" : " 아니다")); box.append(l);
  });
  names.append(box);
});
const editsBox = document.getElementById("edits");
DATA.edits.forEach(e => {
  const box = document.createElement("section"); box.className = "case";
  const h = document.createElement("h2"); h.textContent = `${e.id} · ${e.path}`; box.append(h);
  const p = document.createElement("pre"); p.textContent = e.after || ""; box.append(p);
  e.claims.forEach(c => {
    const row = document.createElement("div"); row.style.margin = "6px 0";
    const t = document.createElement("div"); t.textContent = c.text; row.append(t);
    [["kept", " 담겼다"], ["lost", " 빠졌다"]].forEach(([v, label]) => {
      const l = document.createElement("label"); const r = document.createElement("input");
      r.type = "radio"; r.name = "e-" + c.id; r.value = v; r.checked = state.edits[c.id] === v;
      r.onchange = () => { state.edits[c.id] = v; keep(); };
      l.append(r, document.createTextNode(label)); row.append(l);
    });
    box.append(row);
  });
  editsBox.append(box);
});
if (!DATA.edits.length) document.getElementById("edits-part").hidden = true;
document.getElementById("save").onclick = () => {
  const blob = new Blob([JSON.stringify(state, null, 2)], {type: "application/json"});
  const a = document.createElement("a"); a.href = URL.createObjectURL(blob); a.download = "ingest-labels-answers.json"; a.click();
};
count();
</script></body></html>
""";
}
