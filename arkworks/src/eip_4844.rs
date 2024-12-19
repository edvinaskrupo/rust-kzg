extern crate alloc;

use crate::kzg_proofs::{FFTSettings as LFFTSettings, KZGSettings as LKZGSettings};
use crate::kzg_types::{ArkFp, ArkFr, ArkG1, ArkG1Affine, ArkG2};
use kzg::eip_4844::{
    blob_to_kzg_commitment_rust, compute_blob_kzg_proof_rust, compute_kzg_proof_rust,
    load_trusted_setup_rust, verify_blob_kzg_proof_batch_rust, verify_blob_kzg_proof_rust,
    verify_kzg_proof_rust, Blob, Bytes32, Bytes48, CKZGSettings, KZGCommitment, KZGProof,
    PrecomputationTableManager, BYTES_PER_FIELD_ELEMENT, BYTES_PER_G1, C_KZG_RET,
    C_KZG_RET_BADARGS, C_KZG_RET_OK, FIELD_ELEMENTS_PER_BLOB, FIELD_ELEMENTS_PER_CELL,
    FIELD_ELEMENTS_PER_EXT_BLOB, TRUSTED_SETUP_NUM_G1_POINTS, TRUSTED_SETUP_NUM_G2_POINTS,
};
use kzg::{cfg_into_iter, Fr, G1};

#[cfg(feature = "std")]
use libc::FILE;
#[cfg(feature = "parallel")]
use rayon::prelude::*;
#[cfg(feature = "std")]
use std::fs::File;
#[cfg(feature = "std")]
use std::io::Read;
use std::ptr;

#[cfg(feature = "std")]
use kzg::eip_4844::load_trusted_setup_string;

static mut PRECOMPUTATION_TABLES: PrecomputationTableManager<ArkFr, ArkG1, ArkFp, ArkG1Affine> =
    PrecomputationTableManager::new();

#[cfg(feature = "std")]
pub fn load_trusted_setup_filename_rust(
    filepath: &str,
) -> Result<LKZGSettings, alloc::string::String> {
    let mut file = File::open(filepath).map_err(|_| "Unable to open file".to_string())?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|_| "Unable to read file".to_string())?;

    let (g1_monomial_bytes, g1_lagrange_bytes, g2_monomial_bytes) =
        load_trusted_setup_string(&contents)?;
    load_trusted_setup_rust(&g1_monomial_bytes, &g1_lagrange_bytes, &g2_monomial_bytes)
}

pub(crate) fn fft_settings_to_rust(
    c_settings: *const CKZGSettings,
) -> Result<LFFTSettings, String> {
    let settings = unsafe { &*c_settings };

    let roots_of_unity = unsafe {
        core::slice::from_raw_parts(settings.roots_of_unity, FIELD_ELEMENTS_PER_EXT_BLOB + 1)
            .iter()
            .map(|r| ArkFr::from_blst_fr(*r))
            .collect::<Vec<ArkFr>>()
    };

    let brp_roots_of_unity = unsafe {
        core::slice::from_raw_parts(settings.brp_roots_of_unity, FIELD_ELEMENTS_PER_EXT_BLOB)
            .iter()
            .map(|r| ArkFr::from_blst_fr(*r))
            .collect::<Vec<ArkFr>>()
    };

    let reverse_roots_of_unity = unsafe {
        core::slice::from_raw_parts(
            settings.reverse_roots_of_unity,
            FIELD_ELEMENTS_PER_EXT_BLOB + 1,
        )
        .iter()
        .map(|r| ArkFr::from_blst_fr(*r))
        .collect::<Vec<ArkFr>>()
    };

    Ok(LFFTSettings {
        max_width: FIELD_ELEMENTS_PER_EXT_BLOB,
        root_of_unity: roots_of_unity[0],
        roots_of_unity,
        brp_roots_of_unity,
        reverse_roots_of_unity,
    })
}

pub(crate) fn kzg_settings_to_rust(c_settings: &CKZGSettings) -> Result<LKZGSettings, String> {
    Ok(LKZGSettings {
        fs: fft_settings_to_rust(c_settings)?,
        g1_values_monomial: unsafe {
            core::slice::from_raw_parts(c_settings.g1_values_monomial, FIELD_ELEMENTS_PER_BLOB)
        }
        .iter()
        .map(|r| ArkG1::from_blst_p1(*r))
        .collect::<Vec<_>>(),
        g1_values_lagrange_brp: unsafe {
            core::slice::from_raw_parts(c_settings.g1_values_lagrange_brp, FIELD_ELEMENTS_PER_BLOB)
        }
        .iter()
        .map(|r| ArkG1::from_blst_p1(*r))
        .collect::<Vec<_>>(),
        g2_values_monomial: unsafe {
            core::slice::from_raw_parts(c_settings.g2_values_monomial, TRUSTED_SETUP_NUM_G2_POINTS)
        }
        .iter()
        .map(|r| ArkG2::from_blst_p2(*r))
        .collect::<Vec<_>>(),
        x_ext_fft_columns: unsafe {
            core::slice::from_raw_parts(
                c_settings.x_ext_fft_columns,
                2 * ((FIELD_ELEMENTS_PER_EXT_BLOB / 2) / FIELD_ELEMENTS_PER_CELL),
            )
        }
        .iter()
        .map(|it| {
            unsafe { core::slice::from_raw_parts(*it, FIELD_ELEMENTS_PER_CELL) }
                .iter()
                .map(|it| ArkG1::from_blst_p1(*it))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>(),
        precomputation: unsafe { PRECOMPUTATION_TABLES.get_precomputation(c_settings) },
    })
}

pub(crate) fn kzg_settings_to_c(rust_settings: &LKZGSettings) -> CKZGSettings {
    CKZGSettings {
        roots_of_unity: Box::leak(
            rust_settings
                .fs
                .roots_of_unity
                .iter()
                .map(|r| r.to_blst_fr())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )
        .as_mut_ptr(),
        brp_roots_of_unity: Box::leak(
            rust_settings
                .fs
                .brp_roots_of_unity
                .iter()
                .map(|r| r.to_blst_fr())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )
        .as_mut_ptr(),
        reverse_roots_of_unity: Box::leak(
            rust_settings
                .fs
                .reverse_roots_of_unity
                .iter()
                .map(|r| r.to_blst_fr())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )
        .as_mut_ptr(),
        g1_values_monomial: Box::leak(
            rust_settings
                .g1_values_monomial
                .iter()
                .map(|r| r.to_blst_p1())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )
        .as_mut_ptr(),
        g1_values_lagrange_brp: Box::leak(
            rust_settings
                .g1_values_lagrange_brp
                .iter()
                .map(|r| r.to_blst_p1())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )
        .as_mut_ptr(),
        g2_values_monomial: Box::leak(
            rust_settings
                .g2_values_monomial
                .iter()
                .map(|r| r.to_blst_p2())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )
        .as_mut_ptr(),
        x_ext_fft_columns: Box::leak(
            rust_settings
                .x_ext_fft_columns
                .iter()
                .map(|r| {
                    Box::leak(
                        r.iter()
                            .map(|it| it.to_blst_p1())
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    )
                    .as_mut_ptr()
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )
        .as_mut_ptr(),
        tables: core::ptr::null_mut(),
        wbits: 0,
        scratch_size: 0,
    }
}

unsafe fn deserialize_blob(blob: *const Blob) -> Result<Vec<ArkFr>, C_KZG_RET> {
    (*blob)
        .bytes
        .chunks(BYTES_PER_FIELD_ELEMENT)
        .map(|chunk| {
            let mut bytes = [0u8; BYTES_PER_FIELD_ELEMENT];
            bytes.copy_from_slice(chunk);
            if let Ok(result) = ArkFr::from_bytes(&bytes) {
                Ok(result)
            } else {
                Err(C_KZG_RET_BADARGS)
            }
        })
        .collect::<Result<Vec<ArkFr>, C_KZG_RET>>()
}

macro_rules! handle_ckzg_badargs {
    ($x: expr) => {
        match $x {
            Ok(value) => value,
            Err(_) => return C_KZG_RET_BADARGS,
        }
    };
}

/// # Safety
#[no_mangle]
pub unsafe extern "C" fn blob_to_kzg_commitment(
    out: *mut KZGCommitment,
    blob: *const Blob,
    s: &CKZGSettings,
) -> C_KZG_RET {
    if TRUSTED_SETUP_NUM_G1_POINTS == 0 {
        // FIXME: load_trusted_setup should set this value, but if not, it fails
        TRUSTED_SETUP_NUM_G1_POINTS = FIELD_ELEMENTS_PER_BLOB
    };

    let deserialized_blob = handle_ckzg_badargs!(deserialize_blob(blob));
    let settings = handle_ckzg_badargs!(kzg_settings_to_rust(s));
    let tmp = handle_ckzg_badargs!(blob_to_kzg_commitment_rust(&deserialized_blob, &settings));

    (*out).bytes = tmp.to_bytes();
    C_KZG_RET_OK
}

/// # Safety
#[no_mangle]
pub unsafe extern "C" fn load_trusted_setup(
    out: *mut CKZGSettings,
    g1_monomial_bytes: *const u8,
    num_g1_monomial_bytes: u64,
    g1_lagrange_bytes: *const u8,
    num_g1_lagrange_bytes: u64,
    g2_monomial_bytes: *const u8,
    num_g2_monomial_bytes: u64,
    _precompute: u64,
) -> C_KZG_RET {
    let g1_monomial_bytes =
        core::slice::from_raw_parts(g1_monomial_bytes, num_g1_monomial_bytes as usize);
    let g1_lagrange_bytes =
        core::slice::from_raw_parts(g1_lagrange_bytes, num_g1_lagrange_bytes as usize);
    let g2_monomial_bytes =
        core::slice::from_raw_parts(g2_monomial_bytes, num_g2_monomial_bytes as usize);
    TRUSTED_SETUP_NUM_G1_POINTS = num_g1_monomial_bytes as usize / BYTES_PER_G1;
    let mut settings = handle_ckzg_badargs!(load_trusted_setup_rust(
        g1_monomial_bytes,
        g1_lagrange_bytes,
        g2_monomial_bytes
    ));

    let c_settings = kzg_settings_to_c(&settings);

    PRECOMPUTATION_TABLES.save_precomputation(settings.precomputation.take(), &c_settings);

    *out = c_settings;
    C_KZG_RET_OK
}

/// # Safety
#[cfg(feature = "std")]
#[no_mangle]
pub unsafe extern "C" fn load_trusted_setup_file(
    out: *mut CKZGSettings,
    in_: *mut FILE,
) -> C_KZG_RET {
    let mut buf = vec![0u8; 1024 * 1024];
    let len: usize = libc::fread(buf.as_mut_ptr() as *mut libc::c_void, 1, buf.len(), in_);
    let s = handle_ckzg_badargs!(String::from_utf8(buf[..len].to_vec()));
    let (g1_monomial_bytes, g1_lagrange_bytes, g2_monomial_bytes) =
        handle_ckzg_badargs!(load_trusted_setup_string(&s));
    TRUSTED_SETUP_NUM_G1_POINTS = g1_monomial_bytes.len() / BYTES_PER_G1;
    if TRUSTED_SETUP_NUM_G1_POINTS != FIELD_ELEMENTS_PER_BLOB {
        // Helps pass the Java test "shouldThrowExceptionOnIncorrectTrustedSetupFromFile",
        // as well as 5 others that pass only if this one passes (likely because Java doesn't
        // deallocate its KZGSettings pointer when no exception is thrown).
        return C_KZG_RET_BADARGS;
    }
    let mut settings = handle_ckzg_badargs!(load_trusted_setup_rust(
        &g1_monomial_bytes,
        &g1_lagrange_bytes,
        &g2_monomial_bytes
    ));

    let c_settings = kzg_settings_to_c(&settings);

    PRECOMPUTATION_TABLES.save_precomputation(settings.precomputation.take(), &c_settings);

    *out = c_settings;

    C_KZG_RET_OK
}

/// # Safety
#[no_mangle]
pub unsafe extern "C" fn free_trusted_setup(s: *mut CKZGSettings) {
    if s.is_null() {
        return;
    }

    if !(*s).g1_values_monomial.is_null() {
        let v = Box::from_raw(core::slice::from_raw_parts_mut(
            (*s).g1_values_monomial,
            FIELD_ELEMENTS_PER_BLOB,
        ));
        drop(v);
        (*s).g1_values_monomial = ptr::null_mut();
    }

    if !(*s).g1_values_lagrange_brp.is_null() {
        let v = Box::from_raw(core::slice::from_raw_parts_mut(
            (*s).g1_values_lagrange_brp,
            FIELD_ELEMENTS_PER_BLOB,
        ));
        drop(v);
        (*s).g1_values_lagrange_brp = ptr::null_mut();
    }

    if !(*s).g2_values_monomial.is_null() {
        let v = Box::from_raw(core::slice::from_raw_parts_mut(
            (*s).g2_values_monomial,
            TRUSTED_SETUP_NUM_G2_POINTS,
        ));
        drop(v);
        (*s).g2_values_monomial = ptr::null_mut();
    }

    if !(*s).x_ext_fft_columns.is_null() {
        let v = Box::from_raw(core::slice::from_raw_parts_mut(
            (*s).x_ext_fft_columns,
            2 * ((FIELD_ELEMENTS_PER_EXT_BLOB / 2) / FIELD_ELEMENTS_PER_CELL),
        ));
        drop(v);
        (*s).x_ext_fft_columns = ptr::null_mut();
    }

    if !(*s).roots_of_unity.is_null() {
        let v = Box::from_raw(core::slice::from_raw_parts_mut(
            (*s).roots_of_unity,
            FIELD_ELEMENTS_PER_EXT_BLOB + 1,
        ));
        drop(v);
        (*s).roots_of_unity = ptr::null_mut();
    }

    if !(*s).reverse_roots_of_unity.is_null() {
        let v = Box::from_raw(core::slice::from_raw_parts_mut(
            (*s).reverse_roots_of_unity,
            FIELD_ELEMENTS_PER_EXT_BLOB + 1,
        ));
        drop(v);
        (*s).reverse_roots_of_unity = ptr::null_mut();
    }

    if !(*s).brp_roots_of_unity.is_null() {
        let v = Box::from_raw(core::slice::from_raw_parts_mut(
            (*s).brp_roots_of_unity,
            FIELD_ELEMENTS_PER_EXT_BLOB,
        ));
        drop(v);
        (*s).brp_roots_of_unity = ptr::null_mut();
    }

    PRECOMPUTATION_TABLES.remove_precomputation(&*s);
}

/// # Safety
#[no_mangle]
pub unsafe extern "C" fn verify_kzg_proof(
    ok: *mut bool,
    commitment_bytes: *const Bytes48,
    z_bytes: *const Bytes32,
    y_bytes: *const Bytes32,
    proof_bytes: *const Bytes48,
    s: &CKZGSettings,
) -> C_KZG_RET {
    let frz = handle_ckzg_badargs!(ArkFr::from_bytes(&(*z_bytes).bytes));
    let fry = handle_ckzg_badargs!(ArkFr::from_bytes(&(*y_bytes).bytes));
    let g1commitment = handle_ckzg_badargs!(ArkG1::from_bytes(&(*commitment_bytes).bytes));
    let g1proof = handle_ckzg_badargs!(ArkG1::from_bytes(&(*proof_bytes).bytes));

    let settings = handle_ckzg_badargs!(kzg_settings_to_rust(s));

    let result = handle_ckzg_badargs!(verify_kzg_proof_rust(
        &g1commitment,
        &frz,
        &fry,
        &g1proof,
        &settings
    ));

    *ok = result;
    C_KZG_RET_OK
}

/// # Safety
#[no_mangle]
pub unsafe extern "C" fn verify_blob_kzg_proof(
    ok: *mut bool,
    blob: *const Blob,
    commitment_bytes: *const Bytes48,
    proof_bytes: *const Bytes48,
    s: &CKZGSettings,
) -> C_KZG_RET {
    let deserialized_blob = handle_ckzg_badargs!(deserialize_blob(blob));

    let commitment_g1 = handle_ckzg_badargs!(ArkG1::from_bytes(&(*commitment_bytes).bytes));
    let proof_g1 = handle_ckzg_badargs!(ArkG1::from_bytes(&(*proof_bytes).bytes));

    let settings = handle_ckzg_badargs!(kzg_settings_to_rust(s));

    let result = handle_ckzg_badargs!(verify_blob_kzg_proof_rust(
        &deserialized_blob,
        &commitment_g1,
        &proof_g1,
        &settings,
    ));

    *ok = result;
    C_KZG_RET_OK
}

/// # Safety
#[no_mangle]
pub unsafe extern "C" fn verify_blob_kzg_proof_batch(
    ok: *mut bool,
    blobs: *const Blob,
    commitments_bytes: *const Bytes48,
    proofs_bytes: *const Bytes48,
    n: usize,
    s: &CKZGSettings,
) -> C_KZG_RET {
    let raw_blobs = core::slice::from_raw_parts(blobs, n);
    let raw_commitments = core::slice::from_raw_parts(commitments_bytes, n);
    let raw_proofs = core::slice::from_raw_parts(proofs_bytes, n);

    let deserialized_blobs: Result<Vec<Vec<ArkFr>>, C_KZG_RET> = cfg_into_iter!(raw_blobs)
        .map(|raw_blob| deserialize_blob(raw_blob).map_err(|_| C_KZG_RET_BADARGS))
        .collect();

    let commitments_g1: Result<Vec<ArkG1>, C_KZG_RET> = cfg_into_iter!(raw_commitments)
        .map(|raw_commitment| {
            ArkG1::from_bytes(&raw_commitment.bytes).map_err(|_| C_KZG_RET_BADARGS)
        })
        .collect();

    let proofs_g1: Result<Vec<ArkG1>, C_KZG_RET> = cfg_into_iter!(raw_proofs)
        .map(|raw_proof| ArkG1::from_bytes(&raw_proof.bytes).map_err(|_| C_KZG_RET_BADARGS))
        .collect();

    if let (Ok(blobs), Ok(commitments), Ok(proofs)) =
        (deserialized_blobs, commitments_g1, proofs_g1)
    {
        let settings = match kzg_settings_to_rust(s) {
            Ok(value) => value,
            Err(_) => return C_KZG_RET_BADARGS,
        };

        let result =
            verify_blob_kzg_proof_batch_rust(blobs.as_slice(), &commitments, &proofs, &settings);

        if let Ok(result) = result {
            *ok = result;
            C_KZG_RET_OK
        } else {
            C_KZG_RET_BADARGS
        }
    } else {
        *ok = false;
        C_KZG_RET_BADARGS
    }
}

/// # Safety
#[no_mangle]
pub unsafe extern "C" fn compute_blob_kzg_proof(
    out: *mut KZGProof,
    blob: *const Blob,
    commitment_bytes: *const Bytes48,
    s: &CKZGSettings,
) -> C_KZG_RET {
    let deserialized_blob = match deserialize_blob(blob) {
        Ok(value) => value,
        Err(err) => return err,
    };

    let commitment_g1 = handle_ckzg_badargs!(ArkG1::from_bytes(&(*commitment_bytes).bytes));
    let settings = handle_ckzg_badargs!(kzg_settings_to_rust(s));
    let proof = handle_ckzg_badargs!(compute_blob_kzg_proof_rust(
        &deserialized_blob,
        &commitment_g1,
        &settings
    ));

    (*out).bytes = proof.to_bytes();
    C_KZG_RET_OK
}

/// # Safety
#[no_mangle]
pub unsafe extern "C" fn compute_kzg_proof(
    proof_out: *mut KZGProof,
    y_out: *mut Bytes32,
    blob: *const Blob,
    z_bytes: *const Bytes32,
    s: &CKZGSettings,
) -> C_KZG_RET {
    let deserialized_blob = match deserialize_blob(blob) {
        Ok(value) => value,
        Err(err) => return err,
    };

    let frz = match ArkFr::from_bytes(&(*z_bytes).bytes) {
        Ok(value) => value,
        Err(_) => return C_KZG_RET_BADARGS,
    };

    let settings = match kzg_settings_to_rust(s) {
        Ok(value) => value,
        Err(_) => return C_KZG_RET_BADARGS,
    };

    let (proof_out_tmp, fry_tmp) = match compute_kzg_proof_rust(&deserialized_blob, &frz, &settings)
    {
        Ok(value) => value,
        Err(_) => return C_KZG_RET_BADARGS,
    };

    (*proof_out).bytes = proof_out_tmp.to_bytes();
    (*y_out).bytes = fry_tmp.to_bytes();
    C_KZG_RET_OK
}
