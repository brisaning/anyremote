//! Resampler de mayor orden del audio (plan «único menor» de `evidence/menores-gate.md`
//! y `evidence/audio-gate.md` §Limitaciones): **`rubato`** (puro Rust, ventana
//! Blackman-Harris2) sustituye al **lineal** en captura y reproducción.
//!
//! Por qué: el lineal no filtra contra Nyquist — al **bajar** (p. ej.
//! 48 000 → 44 100 del altavoz) pliega el contenido por encima de la nueva
//! Nyquist (alias audible) y al subir crea imagen; en ratios cercanos el error
//! era «suficiente» (decisión de C-7) pero era el último menor pendiente.
//!
//! Diseño de streaming (los call sites tienen tamaños variables):
//! - **Chunks fijos de entrada** (`FixedSync::Input`): la entrada se acumula en
//!   `pending` y solo se procesa cuando hay un chunk completo; el remanente
//!   (< chunk) queda para la siguiente llamada → nada se pierde entre llamadas.
//! - Latencia añadida: hasta `chunk` muestras de entrada (≈21 ms @48 kHz) +
//!   el retardo propio del FFT (≈ media mitad de bloque, consultable con
//!   `output_delay()`), irrelevante sobre red.
//! - `new` es **infallible para tasas válidas**: `Fft::new` solo rechaza tasas
//!   nulas o chunk 0 (los call sites garantizan `rate != AUDIO_RATE` y el
//!   chunk es una constante).

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

/// Entrada fija por llamada (~21 ms a 48 kHz; sub-chunks por defecto ≈256).
const CHUNK: usize = 1024;

/// Resample i16 `from` → `to` con buffering de chunks (ver módulo).
pub struct SincResampler {
    r: Fft<f32>,
    pending: Vec<f32>,
    out: Vec<f32>,
    chunk: usize,
}

impl SincResampler {
    /// Crea el resampler `from`→`to`. **Invariantes de los call sites**: `from`
    /// y `to` > 0 (tasas de dispositivo/`AUDIO_RATE`) — `Fft::new` solo falla
    /// con ceros o chunk 0, por lo que el error se considera imposible.
    pub fn new(from: u32, to: u32) -> Self {
        let r = Fft::<f32>::new(from as usize, to as usize, CHUNK, 1, FixedSync::Input)
            .unwrap_or_else(|e| unreachable!("tasas no nulas y CHUNK != 0: {e}"));
        let out_cap = r.output_frames_max().max(CHUNK + 1);
        Self {
            r,
            pending: Vec::with_capacity(CHUNK * 2),
            out: vec![0.0; out_cap],
            chunk: CHUNK,
        }
    }

    /// Empuja muestras y devuelve TODO lo que ya esté listo (chunks completos
    /// procesados). El remanente < chunk se conserva internamente.
    pub fn process(&mut self, input: &[i16]) -> Vec<i16> {
        self.pending
            .extend(input.iter().map(|&s| f32::from(s) / 32768.0));
        let mut produced: Vec<i16> = Vec::new();
        while self.pending.len() >= self.chunk {
            let result = {
                // Tres campos disjuntos de `self` prestados a la vez.
                let out_len = self.out.len();
                let input_adapter =
                    InterleavedSlice::new(&self.pending, 1, self.chunk).expect("chunk exacto");
                let mut output_adapter = InterleavedSlice::new_mut(&mut self.out, 1, out_len)
                    .expect("salida con holgura (output_frames_max)");
                self.r
                    .process_into_buffer(&input_adapter, &mut output_adapter, None)
            };
            match result {
                Ok((n_in, n_out)) => {
                    produced.extend(self.out[..n_out].iter().map(|&v| {
                        (v * 32768.0)
                            .round()
                            .clamp(i16::MIN as f32, i16::MAX as f32) as i16
                    }));
                    self.pending.drain(..n_in);
                }
                // Imposible con entrada fija exacta y salida holgada; si
                // ocurriera, se descarta el chunk y se sigue (nunca bucles).
                Err(_) => {
                    self.pending.drain(..self.chunk);
                }
            }
        }
        produced
    }

    /// Retardo inicial en muestras de SALIDA (para alinear en tests/mediciones).
    pub fn output_delay(&self) -> usize {
        self.r.output_delay()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seno de `freq` Hz muestreado a `rate` (i16, amplitud 8000).
    fn sine(freq: f32, rate: u32, n: usize) -> Vec<i16> {
        (0..n)
            .map(|i| {
                let t = i as f32 / rate as f32;
                (8000.0 * (2.0 * std::f32::consts::PI * freq * t).sin()) as i16
            })
            .collect()
    }

    /// Lineal de referencia (el que se retira de producción) para comparar.
    fn linear_ref(input: &[i16], from: u32, to: u32) -> Vec<i16> {
        if from == to || input.is_empty() {
            return input.to_vec();
        }
        let out_len = (input.len() as u64 * to as u64 / from as u64) as usize;
        let ratio = from as f64 / to as f64;
        let last = input.len() - 1;
        (0..out_len)
            .map(|i| {
                let pos = i as f64 * ratio;
                let i0 = (pos as usize).min(last);
                let frac = (pos - i0 as f64) as f32;
                let a = input[i0] as f32;
                let b = input[(i0 + 1).min(last)] as f32;
                (a + (b - a) * frac) as i16
            })
            .collect()
    }

    #[test]
    fn length_bounds_and_silence() {
        // 44.1→48: la salida ≈ entrada × ratio (± retardo/cola del FFT).
        let src = sine(440.0, 44_100, 44_100 / 4);
        let mut up = SincResampler::new(44_100, 48_000);
        let out = up.process(&src);
        let expected = src.len() * 48_000 / 44_100;
        assert!(
            (out.len() as i64 - expected as i64).abs() <= 1500,
            "salida {out_len} vs esperado {expected}",
            out_len = out.len()
        );
        // Sin sobreimpulso: la señal de entrada pico ≈8000 no debe crecer.
        let peak_in = src.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
        let peak_out = out.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
        assert!(
            peak_out <= peak_in + peak_in / 10,
            "pico {peak_out} vs {peak_in}"
        );
        // Silencio → silencio (el FFT no inventa ruido).
        let quiet = SincResampler::new(48_000, 44_100).process(&vec![0i16; 8192]);
        assert!(
            quiet.iter().all(|&s| s.abs() <= 2),
            "ruido con silencio de entrada"
        );
    }

    /// Calidad: al bajar48→44.1 con un seno de5 kHz, el lineal plega/errora
    /// mucho más que el sinc (ventana Blackman-Harris2) — RMSE contra la
    /// onda ideal muestreada A LA FRECUENCIA DE SALIDA, alineando el sinc con
    /// su `output_delay()`.
    #[test]
    fn downsampling_quality_beats_linear() {
        const FREQ: f32 = 5000.0;
        let src = sine(FREQ, 48_000, 48_000_usize / 4); //0.25 s
        let mut sinc = SincResampler::new(48_000, 44_100);
        let sinc_out = sinc.process(&src);
        let ideal = sine(FREQ, 44_100, sinc_out.len());

        let delay = sinc.output_delay().min(sinc_out.len().saturating_sub(1));
        let len = (sinc_out.len() - delay).min(ideal.len());
        let rmse = |a: &[i16], b: &[i16]| -> f64 {
            let n = a.len().min(b.len()) as f64;
            let acc: f64 = a
                .iter()
                .zip(b.iter())
                .map(|(x, y)| {
                    let d = (*x as f64) - (*y as f64);
                    d * d
                })
                .sum();
            (acc / n).sqrt()
        };
        let sinc_err = rmse(&sinc_out[delay..delay + len], &ideal[..len]);

        let lin = linear_ref(&src, 48_000, 44_100);
        let lin_len = lin.len().min(ideal.len());
        let lin_err = rmse(&lin[..lin_len], &ideal[..lin_len]);

        assert!(
            lin_err > 100.0,
            "el lineal debe errar en serio a5 kHz/48→44.1 (err={lin_err:.2}) — si no, la señal no ejercita el alias"
        );
        assert!(
            sinc_err * 4.0 < lin_err,
            "el sinc debe ganar por margen: sinc={sinc_err:.3} vs lineal={lin_err:.3}"
        );
    }
}
