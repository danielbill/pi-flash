import io, ast

p = r'patch_icons.py'
lines = io.open(p, encoding='utf-8').read().splitlines(keepends=True)

# locate the rep whose body contains "u{2699}") + continuation with model_label
start = None
for i, l in enumerate(lines):
    if 'u{2699}' in l and l.lstrip().startswith("rep('"):
        start = i
        break
assert start is not None
# block is start..start+2 (rep line, continuation line with model_label),)
assert '.child(model_label),' in lines[start + 1]

bs = chr(92)
nl_lit = bs + 'n'
line1 = "rep('                                            .child(\"' + bs + 'u{2699}\")" + nl_lit
line2 = "                                            .child(model_label),',\n"
lines[start] = line1
lines[start + 1] = line2

io.open(p, 'w', encoding='utf-8', newline='\n').write(''.join(lines))
ast.parse(io.open(p, encoding='utf-8').read())
print('line fixed, script syntax OK')
