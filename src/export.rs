use hound;
use crate::state::Project;

pub fn export_wav(project: &Project, path: &str) {
    let spec = hound::WavSpec {
        channels: project.channels,
        sample_rate: project.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = hound::WavWriter::create(path, spec).unwrap();

    for seg in &project.segments {
        for &sample in &seg.samples {
            let s = (sample * i16::MAX as f32) as i16;
            writer.write_sample(s).unwrap();
        }
    }

    writer.finalize().unwrap();
}
