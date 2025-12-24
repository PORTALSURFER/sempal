param(
    [string]$ProfilePath = ".\profile.json.gz"
)

$py = @'
import gzip, json, sys, collections

path = sys.argv[1]
open_fn = gzip.open if path.endswith(".gz") else open
with open_fn(path, "rt", encoding="utf-8") as f:
    prof = json.load(f)

thr = prof["threads"][0]
samples = thr["samples"]

def col(table, name):
    if "data" in table and "schema" in table:
        return table["data"][table["schema"].index(name)]
    return table[name]

stack_col = col(samples, "stack")
stack_table = thr["stackTable"]
frame_table = thr["frameTable"]
func_table = thr["funcTable"]
string_array = thr["stringArray"]

stack_frame = col(stack_table, "frame")
frame_func = col(frame_table, "func")
func_name = col(func_table, "name")

counts = collections.Counter()
total = 0

for sid in stack_col:
    if sid is None:
        continue
    total += 1
    fid = stack_frame[sid]
    func_id = frame_func[fid]
    name = string_array[func_name[func_id]]
    counts[name] += 1

print(f"Total samples: {total}")
for name, count in counts.most_common(30):
    pct = 100.0 * count / total if total else 0.0
    print(f"{pct:6.2f}%  {count:8d}  {name}")
'@

$py | python - $ProfilePath
