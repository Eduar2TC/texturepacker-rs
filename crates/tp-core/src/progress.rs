//! Progreso de una corrida, para la barra de la tira de estado.
//!
//! La tira enseña dos cosas distintas mientras se empaqueta y ninguna sirve
//! sola:
//!
//! * El recuento de imágenes que lleva cargadas la ingesta
//!   («Empaquetando… 3/13»). Se queda clavado en «13/13» en cuanto acaba la
//!   ingesta, que es el primer tramo de la corrida.
//! * La fracción de la corrida **entera** que ya está hecha: sigue moviéndose
//!   durante el empaquetado y la escritura, pero sola no dice qué se está
//!   cargando.
//!
//! La barra se reparte en tramos de milésimas: cada fase ocupa un tramo fijo
//! que decide quien la lanza, y dentro del tramo avanza según sus propios
//! trabajos (imágenes cargadas, sub-etapas, páginas escritas). De ahí dos
//! propiedades que la tira necesita:
//!
//! * **No puede ir hacia atrás.** Entre fases los tramos son contiguos (el
//!   `start + len` de uno es el `start` del siguiente), así que arrancar una
//!   fase nueva sólo puede empujar la barra hacia delante, aunque la fase
//!   anterior se haya quedado corta o se haya dado por hecha sin trabajos.
//! * **Termina en el borde.** El último tramo cierra en1000 milésimas, de
//!   modo que con todos sus trabajos avisados la fracción es 1.
//!
//! El objeto vive en un `Arc`: lo crea la interfaz al lanzar la publicación,
//! lo lleva el hilo de la corrida y la tira lo lee mientras esté puesto.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// Porción de la barra: `len` milésimas empezando en la milésima `start`.
///
/// Milésimas y no porcentaje porque los pesos de las fases no reparten huecos
/// enteros: cinco fases con pesos distintos no cortan100 baldosas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub start: u32,
    pub len: u32,
}

impl Segment {
    /// La barra entera: por donde arranca una corrida de arriba del todo.
    pub const FULL: Segment = Segment {
        start: 0,
        len: 1000,
    };

    /// La barra de `root` partida en tantos tramos como pesos.
    ///
    /// Los bordes salen de la suma acumulada y se redondean una sola vez, así
    /// que los tramos son contiguos entre sí y el último cierra exactamente
    /// en el borde de `root`. Esa contigüidad es lo que impide que la barra
    /// retroceda al cambiar de fase.
    pub fn split(root: Segment, weights: &[u32]) -> Vec<Segment> {
        let total: u64 = weights.iter().map(|w| u64::from(*w)).sum();
        let mut segments = Vec::with_capacity(weights.len());
        let mut accumulated: u64 = 0;
        let mut start = root.start;
        for (i, weight) in weights.iter().enumerate() {
            accumulated += u64::from(*weight);
            let end = if i + 1 == weights.len() || total == 0 {
                root.start.saturating_add(root.len)
            } else {
                root.start
                    .saturating_add(((u64::from(root.len) * accumulated) / total) as u32)
            };
            segments.push(Segment {
                start,
                len: end.saturating_sub(start),
            });
            start = end;
        }
        segments
    }
}

/// Progreso de una corrida.
///
/// Sólo escribe quien ejecuta la fase y sólo lee la tira, así que un
/// orden relajado basta: lo que se pierde es como mucho un frame de retraso
/// en la barra.
#[derive(Debug, Default)]
pub struct Progress {
    /// Milésimas donde empieza la fase actual.
    start: AtomicU32,
    /// Milésimas que ocupa la fase actual.
    len: AtomicU32,
    /// Trabajos terminados de la fase actual.
    done: AtomicUsize,
    /// Trabajos que tiene la fase actual. Con0 la fase se da por hecha en
    /// cuanto arranca (una fase sin variantes, por ejemplo).
    total: AtomicUsize,
    /// Imágenes que la ingesta ya ha cargado (numerador del contador).
    loaded: AtomicUsize,
    /// Imágenes que la ingesta ha descubierto (denominador del contador).
    files: AtomicUsize,
}

impl Progress {
    pub fn new() -> Self {
        Self::default()
    }

    /// Empieza la fase que ocupa `segment` y contiene `items` trabajos.
    ///
    /// El tramo se fija aquí y no se mueve: los de una corrida son contiguos,
    /// así que esta llamada sólo puede hacer avanzar la barra.
    pub fn begin_phase(&self, segment: Segment, items: usize) {
        self.start.store(segment.start, Ordering::Relaxed);
        self.len.store(segment.len, Ordering::Relaxed);
        self.total.store(items, Ordering::Relaxed);
        self.done.store(0, Ordering::Relaxed);
    }

    /// Fija el total de trabajos de la fase cuando se conoce tarde: la
    /// ingesta sólo sabe cuántas imágenes hay al terminar el descubrimiento.
    pub fn set_total(&self, total: usize) {
        self.total.store(total, Ordering::Relaxed);
    }

    /// Trabajos terminados en la fase actual. Pasarse del total no se nota:
    /// la fracción de la fase se queda topeada en 1.
    pub fn add(&self, items: usize) {
        self.done.fetch_add(items, Ordering::Relaxed);
    }

    /// Imágenes que la ingesta va a cargar: el denominador del contador.
    pub fn set_files(&self, files: usize) {
        self.files.store(files, Ordering::Relaxed);
    }

    /// Una imagen cargada más (numerador del contador).
    pub fn add_loaded(&self) {
        self.loaded.fetch_add(1, Ordering::Relaxed);
    }

    /// `(cargadas, descubiertas)` para el texto de la tira.
    pub fn loaded(&self) -> (usize, usize) {
        (
            self.loaded.load(Ordering::Relaxed),
            self.files.load(Ordering::Relaxed),
        )
    }

    /// Fracción0..1 de la corrida entera.
    ///
    /// Con la fase sin total se da por hecha; sin ninguna fase anunciada la
    /// barra está a cero (el tramo todavía no ocupa nada).
    pub fn fraction(&self) -> f32 {
        let total = self.total.load(Ordering::Relaxed);
        let done = self.done.load(Ordering::Relaxed).min(total);
        let inside = if total == 0 {
            1.0
        } else {
            done as f32 / total as f32
        };
        let start = self.start.load(Ordering::Relaxed) as f32;
        let len = self.len.load(Ordering::Relaxed) as f32;
        ((start + len * inside) / Segment::FULL.len as f32).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Los tramos tienen que cubrir la barra sin solaparse ni dejar huecos:
    /// un hueco es un sitio desde el que la barra podría retroceder al
    /// cambiar de fase.
    #[test]
    fn los_tramos_cubren_la_barra_sin_huecos() {
        for pesos in [
            vec![350, 100, 150, 200, 200],
            vec![350, 650],
            vec![1, 1, 1],
            vec![7, 3],
        ] {
            let tramos = Segment::split(Segment::FULL, &pesos);
            assert_eq!(tramos.len(), pesos.len(), "pesos: {pesos:?}");
            let mut esperado = Segment::FULL.start;
            for tramo in &tramos {
                assert_eq!(
                    tramo.start, esperado,
                    "hueco o solape entre tramos de {pesos:?}: {tramos:?}"
                );
                esperado = tramo.start + tramo.len;
            }
            assert_eq!(
                esperado,
                Segment::FULL.start + Segment::FULL.len,
                "el último tramo debe cerrar la barra: {pesos:?}"
            );
        }
    }

    /// Recorrido de una corrida de cinco fases: la barra nunca baja y al
    /// terminar la última está en el borde.
    #[test]
    fn la_barra_no_vuelve_atras_y_acaba_en_el_borde() {
        let progreso = Progress::new();
        let tramos = Segment::split(Segment::FULL, &[350, 100, 150, 200, 200]);
        let mut anterior = progreso.fraction();
        assert_eq!(anterior, 0.0, "sin fase anunciada la barra está a cero");
        let mut mira = |mensaje: String| {
            let ahora = progreso.fraction();
            assert!(
                ahora >= anterior,
                "la barra retrocedió de {anterior} a {ahora}: {mensaje}"
            );
            anterior = ahora;
            ahora
        };

        progreso.begin_phase(tramos[0], 100);
        progreso.set_files(13);
        progreso.add_loaded();
        progreso.add_loaded();
        progreso.add(30);
        mira("un tercio de la ingesta".into());
        progreso.add(70);
        let fin_ingesta = mira("ingesta terminada".into());
        assert!((fin_ingesta - 0.35).abs() < 1e-6);

        // Fase sin trabajos: se da por hecha y la barra salta a su borde.
        progreso.begin_phase(tramos[1], 0);
        let sin_variantes = mira("sin variantes que correr".into());
        assert!((sin_variantes - 0.45).abs() < 1e-6);

        progreso.begin_phase(tramos[2], 4);
        progreso.add(2);
        mira("composición a medias".into());
        // Un aviso de más no empuja la barra fuera de su tramo.
        progreso.add(500);
        let fin_composicion = mira("avisos de más".into());
        assert!((fin_composicion - 0.60).abs() < 1e-6);

        // Una fase que se queda corta (un `?` a mitad de camino) no frena a
        // la siguiente: al anunciarla la barra salta a su propio tramo.
        progreso.begin_phase(tramos[3], 10);
        progreso.add(1);
        let corta = mira("fase interrumpida".into());
        assert!(
            (corta - 0.62).abs() < 1e-6,
            "1 de 10 trabajos en el cuarto tramo: {corta}"
        );
        progreso.begin_phase(tramos[4], 3);
        progreso.add(3);
        let fin = mira("escritura terminada".into());
        assert!(corta < fin, "la fase siguiente debe avanzar la barra");
        assert_eq!(fin, 1.0, "con la última fase cerrada la barra es 1");
        assert_eq!(
            progreso.loaded(),
            (2, 13),
            "el contador de imágenes no lo mueve la barra ni al revés"
        );
    }

    /// El total de la ingesta sólo se sabe al descubrir los ficheros, que
    /// es después de haber anunciado la fase. Hasta entonces no hay
    /// trabajos y la fase no debe quedarse muerta en el 0 %.
    #[test]
    fn el_total_que_se_conoce_tarde_mueve_la_barra() {
        let progreso = Progress::new();
        progreso.begin_phase(Segment::FULL, 0);
        assert_eq!(
            progreso.fraction(),
            1.0,
            "una fase aún sin total se da por hecha"
        );
        progreso.set_total(10);
        progreso.add(4);
        assert!(
            (progreso.fraction() - 0.4).abs() < 1e-6,
            "con total fijado, 4 de 10: {}",
            progreso.fraction()
        );
        progreso.add(10_000);
        assert_eq!(
            progreso.fraction(),
            1.0,
            "los trabajos de más no sacan la barra del tramo"
        );
    }
}
