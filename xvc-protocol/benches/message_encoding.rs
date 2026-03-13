use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use xvc_protocol::Message;

fn criterion_benchmark(c: &mut Criterion) {
    let message = Message::GetInfo;

    c.bench_with_input(
        BenchmarkId::new("message", "getinfo"),
        &message,
        |b, msg| {
            b.iter(|| {
                let mut writer = Vec::new();
                msg.write_to(&mut writer).expect("Cannot write message");
                writer
            })
        },
    );

    let message = Message::SetTck { period_ns: 100 };

    c.bench_with_input(BenchmarkId::new("message", "settck"), &message, |b, msg| {
        b.iter(|| {
            let mut writer = Vec::new();
            msg.write_to(&mut writer).expect("Cannot write message");
            writer
        })
    });

    let tdi = vec![0xAAu8; 128];
    let tms = vec![0x55; 128];
    let num_bits = (tdi.len() * 8) as u32;

    let message = Message::Shift {
        num_bits,
        tms: tms.into_boxed_slice(),
        tdi: tdi.into_boxed_slice(),
    };

    c.bench_with_input(BenchmarkId::new("message", "shift"), &message, |b, msg| {
        b.iter(|| {
            let mut writer = Vec::new();
            msg.write_to(&mut writer).expect("Cannot write message");
            writer
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
