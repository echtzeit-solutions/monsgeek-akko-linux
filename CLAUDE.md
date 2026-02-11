IMPORTANT avoid reading from hidraw devices as it will lock the claude process
IMPORTANT avoid printing the full device path in printf to avoid claude from speculatively reading from it
Use `tmux send-keys -t 3.2 'command' Enter` to run commands as root in dev pane (already sudo bash)

## ry_upgrade.exe stdout
- "CONTENT []" and "split err" are from netstat/competing process checking, NOT device enumeration
- "开启vendor监听!!!!!!" = "Starting vendor monitoring!" = app found & paired IF1+IF2

## BPF Loading Notes

**akko-loader vs bpftool:**
- Our BPF is loaded with aya (Rust), not libbpf/bpftool
- bpftool fails on our BPF object due to multiple .ksyms sections (aya-ebpf emits separate .ksyms per kfunc)
- This is acceptable - akko-loader works; bpftool compatibility is optional improvement
- Error when using bpftool: "failed to find BTF for extern 'hid_bpf_get_data'" due to multiple .ksyms sections

**struct_ops BTF Requirements:**
- Kernel validates BPF object's BTF against vmlinux during map creation
- Function pointer fields should be PTR -> FUNC_PROTO in BTF
- Rust generates PTR -> INT '()' size=0 for `*const ()` which kernel may reject
- May need aya-ebpf changes to generate proper FUNC_PROTO types