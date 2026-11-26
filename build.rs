//! Emit a standard x64 COFF resource section. No rc.exe, C compiler, or SDK needed.
use object::{
    write::{Object, Relocation},
    Architecture, BinaryFormat, Endianness, RelocationFlags, SectionKind,
};
use std::{collections::BTreeMap, env, fs, path::PathBuf};

fn u16at(a: &[u8], i: usize) -> usize {
    u16::from_le_bytes(a[i..i + 2].try_into().unwrap()) as usize
}
fn u32at(a: &[u8], i: usize) -> usize {
    u32::from_le_bytes(a[i..i + 4].try_into().unwrap()) as usize
}
fn put(a: &mut [u8], i: usize, n: usize) {
    a[i..i + 4].copy_from_slice(&(n as u32).to_le_bytes());
}
fn directory(b: &mut Vec<u8>, n: usize) -> usize {
    let p = b.len();
    b.resize(p + 16 + 8 * n, 0);
    b[p + 14..p + 16].copy_from_slice(&(n as u16).to_le_bytes());
    p
}
fn entry(b: &mut [u8], dir: usize, i: usize, id: usize, off: usize) {
    put(b, dir + 16 + 8 * i, id);
    put(b, dir + 20 + 8 * i, off);
}
fn main() {
    println!("cargo:rerun-if-changed=assets/relay.ico");
    println!("cargo:rerun-if-changed=src/relay.manifest");
    let ico = fs::read("assets/relay.ico").unwrap();
    let n = u16at(&ico, 4);
    let mut resources: BTreeMap<usize, Vec<(usize, Vec<u8>)>> = BTreeMap::new();
    let mut group = ico[..6].to_vec();
    for i in 0..n {
        let p = 6 + 16 * i;
        let size = u32at(&ico, p + 8);
        let at = u32at(&ico, p + 12);
        group.extend_from_slice(&ico[p..p + 12]);
        group.extend_from_slice(&((i + 1) as u16).to_le_bytes());
        resources
            .entry(3)
            .or_default()
            .push((i + 1, ico[at..at + size].to_vec()));
    }
    resources.insert(14, vec![(1, group)]);
    resources.insert(24, vec![(1, fs::read("src/relay.manifest").unwrap())]);
    let mut data = Vec::new();
    let root = directory(&mut data, resources.len());
    let mut fix = Vec::new();
    for (ti, (kind, items)) in resources.into_iter().enumerate() {
        let ty = directory(&mut data, items.len());
        entry(&mut data, root, ti, kind, 0x80000000 | ty);
        for (j, (id, value)) in items.into_iter().enumerate() {
            let lang = directory(&mut data, 1);
            entry(&mut data, ty, j, id, 0x80000000 | lang);
            let d = data.len();
            entry(&mut data, lang, 0, 1033, d);
            data.resize(d + 16, 0);
            put(&mut data, d + 4, value.len());
            let pos = data.len();
            fix.push((d, pos));
            data.extend(value);
            while data.len() % 4 != 0 {
                data.push(0);
            }
        }
    }
    let mut obj = Object::new(BinaryFormat::Coff, Architecture::X86_64, Endianness::Little);
    let sec = obj.add_section(vec![], b".rsrc".to_vec(), SectionKind::ReadOnlyData);
    obj.append_section_data(sec, &data, 4);
    let sym = obj.section_symbol(sec);
    for (d, at) in fix {
        obj.add_relocation(
            sec,
            Relocation {
                offset: d as u64,
                symbol: sym,
                addend: at as i64,
                flags: RelocationFlags::Coff { typ: 3 },
            },
        )
        .unwrap();
    }
    let path = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("relay-res.obj");
    fs::write(&path, obj.write().unwrap()).unwrap();
    println!("cargo:rustc-link-arg-bin=Relay={}", path.display());
}
