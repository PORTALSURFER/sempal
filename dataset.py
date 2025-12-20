import os, collections
root = r"D:\music-production\samples\trainingset"
counts = collections.Counter()
for name in os.listdir(root):
  p = os.path.join(root, name)
  if os.path.isdir(p):
      cnt = sum(1 for _ in os.scandir(p) if _.is_file())
      counts[name] = cnt
for k,v in counts.most_common():
  print(f"{k}: {v}")
