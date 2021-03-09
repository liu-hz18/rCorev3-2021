import os
import sys

chapter_test = '' if len(sys.argv) < 2 else sys.argv[1] 

base_address = 0x80400000
step = 0x20000
linker = 'src/linker.ld'

app_id = 0
apps = os.listdir('src/bin')
apps.sort()
for app in apps:
    if app.startswith('_'):
        continue
    if chapter_test != '' and not app.startswith(chapter_test):
        continue
    app = app[:app.find('.')]
    lines = []
    lines_before = []
    # 找到 src/linker.ld 中的 BASE_ADDRESS = 0x80100000; 这一行，并将后面的地址 替换为和当前应用对应的一个地址
    with open(linker, 'r') as f:
        for line in f.readlines():
            lines_before.append(line)
            line = line.replace(hex(base_address), hex(base_address+step*app_id))
            lines.append(line)
    with open(linker, 'w+') as f:
        f.writelines(lines)
    # 使用 cargo build 构建当前的应用，注意我们可以使用 --bin 参数来只构建某一个应用
    os.system('cargo build --bin %s --release' % app)
    print('[build.py] application %s start with address %s' %(app, hex(base_address+step*app_id)))
    # 将 src/linker.ld 还原
    with open(linker, 'w+') as f:
        f.writelines(lines_before)
    app_id = app_id + 1
