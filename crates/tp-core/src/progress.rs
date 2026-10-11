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
//! * **Termina en el borde de salida.** El último tramo cierra en 1000
//!   milésimas, de modo que con todos sus trabajos avisados la fracción es 1.
//!
//! El objeto vive en un `Arc`: lo crea la interfaz al lanzar la publicación,
//! lo lleva el hilo de la corrida y la tira lo lee mientras esté puesto.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// Porción de la barra: `len` milésimas empezando en la milésima `start`.
///
/// Milésimas y no porcentaje porque los pesos de las fases no reparten huecos
/// enteros: cinco fases con pesos distintos no cortan 100 baldosas.
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

/// Estado mutable de la barra, leído y escrito siempre entero.
#[derive(Debug, Default)]
struct Estado {
    /// Milésimas donde empieza la fase actual.
    start: u32,
    /// Milésimas que ocupa la fase actual.
    len: u32,
    /// Trabajos terminados de la fase actual.
    done: usize,
    /// Trabajos que tiene la fase actual.
    total: usize,
    /// `true` cuando el total de la fase ya se conoce; mientras es `false`
    /// la fase está anunciada pero sin contar (las dos primeras descubren su
    /// total al escanear los ficheros y al planificar las variantes).
    ///
    /// Es la diferencia entre «esta fase no tiene nada que hacer» (total 0 y
    /// conocido: la barra cierra el tramo de un golpe) y «esta fase aún no
    /// sabe cuánto hará» (total 0 sin conocer: la barra se queda parada en el
    /// borde de entrada a la espera). Sin distinguirlas la barra arrancaba
    /// subiendo al borde de salida del primer tramo (35 %) y, en cuanto la
    /// ingesta fijaba el total, caía de golpe a 0 (re-review B2).
    conocido: bool,
    /// Imágenes que la ingesta ya ha cargado (numerador del contador).
    loaded: usize,
    /// Imágenes que la ingesta ha descubierto (denominador del contador).
    files: usize,
}

/// Progreso de una corrida.
///
/// **Los siete campos van juntos en un `Mutex` y no en átomos sueltos.** La
/// tira lee desde otro hilo mientras la corrida escribe, y con átomos
/// independientes la lectura se hacía a caballo de dos fases: `fraction()`
/// cogía el «total terminado» de la anterior y el tramo de la siguiente, veía
/// así la barra al borde de salida de una fase que aún no había empezado
/// (0.45) y, en la lectura siguiente, caía al borde de entrada (0.35). Un
/// paso atrás en pantalla por una ventana de nanoseundos que la corrida real
/// alcanzó en CI (re-review B2). Ningún orden de escritura lo arregla: hace
/// falta que la lectora vea un retrato entero y coherente, y para eso está
/// el candado —se secciona durante nanoseundos y sólo lo lee o escribe quien
/// lo tiene.
#[derive(Debug, Default)]
pub struct Progress {
    estado: Mutex<Estado>,
}

impl Progress {
    pub fn new() -> Self {
        Self::default()
    }

    /// El estado, con el pánico de otro hilo ignorado: los campos son
    /// contadores que siguen siendo válidos aunque quien los fuera tocando
    /// se cayera, y una barra de progreso no puede quedarse inservible.
    fn estado(&self) -> MutexGuard<'_, Estado> {
        self.estado.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Empieza la fase que ocupa `segment` y contiene `items` trabajos.
    ///
    /// El tramo se fija aquí y no se mueve: los de una corrida son contiguos,
    /// así que esta llamada sólo puede hacer avanzar la barra.
    ///
    /// `items == 0` se interpreta como «esta fase todavía no sabe cuántos
    /// trabajos tiene»: la barra queda parada en el borde de entrada del
    /// tramo hasta que [`Progress::set_total`] la llene (re-review B2).
    /// Quien ya conoce su recuento desde el principio debe pasarlo aquí y
    /// no llamar a `set_total`.
    pub fn begin_phase(&self, segment: Segment, items: usize) {
        let mut e = self.estado();
        e.start = segment.start;
        e.len = segment.len;
        e.total = items;
        e.done = 0;
        e.conocido = items > 0;
    }

    /// Fija el total de trabajos de la fase cuando se conoce tarde: la
    /// ingesta sólo sabe cuántas imágenes hay al terminar el descubrimiento.
    ///
    /// Hasta esta llamada la fase se queda **parada en el borde de entrada**
    /// de su tramo, sin avanzar ni retroceder (re-review B2).
    pub fn set_total(&self, total: usize) {
        let mut e = self.estado();
        e.total = total;
        e.conocido = true;
    }

    /// Trabajos terminados en la fase actual. Pasarse del total no se nota:
    /// la fracción de la fase se queda topeada en 1.
    pub fn add(&self, items: usize) {
        self.estado().done += items;
    }

    /// Imágenes que la ingesta va a cargar: el denominador del contador.
    pub fn set_files(&self, files: usize) {
        self.estado().files = files;
    }

    /// Una imagen cargada más (numerador del contador).
    pub fn add_loaded(&self) {
        self.estado().loaded += 1;
    }

    /// `(cargadas, descubiertas)` para el texto de la tira.
    pub fn loaded(&self) -> (usize, usize) {
        let e = self.estado();
        (e.loaded, e.files)
    }

    /// Fracción 0..1 de la corrida entera.
    ///
    /// Una fase cuyo total aún no se conoce queda **parada en el borde de
    /// entrada** de su tramo: no avanza, pero tampoco retrocede. Es lo que
    /// impide el salto de 35 % a 0 del que adolecía el código anterior
    /// (re-review B2), que trataba «total sin conocer» como «fase hecha».
    ///
    /// El retrato sale de una sola sección: mezclar el total de una fase con
    /// el tramo de la siguiente es lo que hacía bajar la barra (ver
    /// [`Progress`]).
    pub fn fraction(&self) -> f32 {
        let e = self.estado();
        let inside = if !e.conocido {
            0.0
        } else if e.total == 0 {
            1.0
        } else {
            e.done.min(e.total) as f32 / e.total as f32
        };
        ((e.start as f32 + e.len as f32 * inside) / Segment::FULL.len as f32).clamp(0.0, 1.0)
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

        // Fase sin trabajos: se anuncia con 0 y se queda parada en su
        // borde de entrada hasta que el total la llene (B2). Sólo entonces
        // avanza, y nunca retrocede.
        progreso.begin_phase(tramos[1], 0);
        let sin_variantes = mira("anunciada sin total".into());
        assert!((sin_variantes - 0.35).abs() < 1e-6);
        progreso.set_total(10);
        let total_fijado = mira("con el total fijado".into());
        assert!(
            total_fijado >= sin_variantes,
            "fijar el total no puede hacer bajar la barra"
        );
        progreso.add(10);
        let fin_variantes = mira("variantes terminadas".into());
        assert!((fin_variantes - 0.45).abs() < 1e-6);

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
    /// es después de haber anunciado la fase. Hasta entonces la barra se
    /// queda **parada en el borde de entrada** del tramo, sin avanzar ni
    /// retroceder: si saltara al borde de salida (como hacía antes), fijar
    /// el total la haría caer de golpe, y la barra no puede ir hacia atrás.
    #[test]
    fn el_total_que_se_conoce_tarde_no_hace_bajar_la_barra() {
        let progreso = Progress::new();
        progreso.begin_phase(Segment::FULL, 0);
        assert_eq!(
            progreso.fraction(),
            0.0,
            "una fase aún sin total se queda en su borde de entrada"
        );
        progreso.set_total(10);
        assert_eq!(
            progreso.fraction(),
            0.0,
            "fijar el total no puede hacer bajar la barra"
        );
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

    /// La tira lee desde otro hilo mientras la corrida escribe, y con campos
    /// sueltos la lectura salía a caballo de dos fases: tomaba el «todo
    /// terminado» de la anterior y el tramo de la siguiente, veía la barra en
    /// el borde de salida de una fase que aún no había empezado y, en la
    /// lectura siguiente, caía a su borde de entrada. Un paso atrás, que es
    /// exactamente lo que la corrida real cazó en CI (re-review B2).
    ///
    /// Aquí se reproduce el mismo baile a escala: un hilo cierra una fase y
    /// abre la siguiente mientras el otro no deja de leer. Con la barra
    /// partida en 500 tramos de 2 milésimas, cada traspaso tiene su ventana
    /// de dos milésimas de salto —4 en lugar de 2— seguida de la caída.
    #[test]
    fn la_barra_no_retrocede_aunque_lo_lea_otro_hilo() {
        const FASES: usize = 500;
        let progreso = std::sync::Arc::new(Progress::new());
        let pesos = vec![1u32; FASES];
        let tramos = Segment::split(Segment::FULL, &pesos);
        assert_eq!(tramos.len(), FASES);
        assert!(tramos.iter().all(|t| t.len >= 1), "cada tramo tiene carne");

        let terminado = std::sync::atomic::AtomicBool::new(false);
        let (retrocesos, escrito) = std::thread::scope(|s| {
            let lector = s.spawn(|| {
                let mut maximo = 0.0f32;
                let mut pasos_atras = Vec::new();
                while !terminado.load(std::sync::atomic::Ordering::Relaxed) {
                    // 64 lecturas seguidas y una rendición: seguidas para
                    // que la ventana de nanoseundos del traspaso no se le
                    // escape, rendidas para que en una máquina de un solo
                    // núcleo el hilo que escribe también corra.
                    for _ in 0..64 {
                        let ahora = progreso.fraction();
                        if ahora < maximo - 1e-6 && pasos_atras.len() < 3 {
                            pasos_atras.push((maximo, ahora));
                        }
                        maximo = maximo.max(ahora);
                    }
                    std::thread::yield_now();
                }
                pasos_atras
            });
            let escritor = s.spawn(|| {
                for tramo in tramos {
                    progreso.begin_phase(tramo, 10);
                    // Pausa tras anunciar: es donde la otra hilo tiene que
                    // coger el estado ya asentado de la fase nueva.
                    std::thread::yield_now();
                    progreso.add(10);
                    std::thread::yield_now();
                }
                terminado.store(true, std::sync::atomic::Ordering::Relaxed);
            });
            let resultado_escritor = escritor.join();
            // Pase lo que pase con el escritor, el lector tiene que salir:
            // si el hilo se cayera, la bandera nunca se levantaría y el
            // scope esperaría a una lectura que no termina.
            terminado.store(true, std::sync::atomic::Ordering::Relaxed);
            let pasos = lector.join().expect("el lector debe terminar");
            resultado_escritor.expect("el escritor debe terminar");
            (pasos, progreso.clone())
        });

        assert!(
            retrocesos.is_empty(),
            "la barra retrocedió al leerla desde otro hilo: {retrocesos:?}"
        );
        assert_eq!(
            escrito.fraction(),
            1.0,
            "cerradas las 500 fases la barra está en el borde"
        );
    }
}
