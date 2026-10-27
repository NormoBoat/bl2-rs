//! Research helper: prints export headers and the first bytes of their bodies.
//! cargo run -p bl2-formats --example peek -- <package.upk> [first_export] [count] [bytes] [class_filter]

use bl2_formats::package::Package;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let p = Package::open(args[1].as_ref()).expect("open package");
    let first: usize = args.get(2).map_or(0, |s| s.parse().unwrap());
    let count: usize = args.get(3).map_or(20, |s| s.parse().unwrap());
    let nbytes: usize = args.get(4).map_or(48, |s| s.parse().unwrap());
    let obj = |i: i32| match i {
        0 => "None".to_string(),
        i if i > 0 => p.name(p.exports[i as usize - 1].object_name),
        i => {
            let imp = &p.imports[(-i - 1) as usize];
            format!("{}.{}", p.name(imp.class_name), p.name(imp.object_name))
        }
    };
    let filter = args.get(5).cloned().unwrap_or_default();
    let names = |i: i32| if i < 0 { p.name(p.imports[(-i - 1) as usize].object_name) } else { obj(i) };
    for (i, e) in p.exports.iter().enumerate().skip(first).filter(|(_, e)| names(e.class) == filter || filter.is_empty()).take(count) {
        let body = &p.data[e.serial_offset..e.serial_offset + e.serial_size];
        let hex: Vec<String> = body.iter().take(nbytes).map(|b| format!("{b:02x}")).collect();
        let outer_class = if e.outer > 0 { names(p.exports[e.outer as usize - 1].class) } else { "-".into() };
        println!(
            "#{i} {} class={} outer={} (class {outer_class}) flags={:#018x} size={}\n    {}",
            p.name(e.object_name), obj(e.class), obj(e.outer), e.object_flags, e.serial_size, hex.join(" ")
        );
    }
}
