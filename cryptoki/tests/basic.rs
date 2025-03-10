// Copyright 2021 Contributors to the Parsec project.
// SPDX-License-Identifier: Apache-2.0
mod common;

use crate::common::{SO_PIN, USER_PIN};
use common::init_pins;
use cryptoki::error::{Error, RvError};
use cryptoki::mechanism::aead::GcmParams;
use cryptoki::mechanism::Mechanism;
use cryptoki::object::{Attribute, AttributeInfo, AttributeType, KeyType, ObjectClass};
use cryptoki::session::{SessionState, UserType};
use cryptoki::types::AuthPin;
use serial_test::serial;
use std::collections::HashMap;
use std::thread;

use testresult::TestResult;

#[test]
#[serial]
fn sign_verify() -> TestResult {
    let (pkcs11, slot) = init_pins();

    // open a session
    let session = pkcs11.open_rw_session(slot)?;

    // log in the session
    session.login(UserType::User, Some(&AuthPin::new(USER_PIN.into())))?;

    // get mechanism
    let mechanism = Mechanism::RsaPkcsKeyPairGen;

    let public_exponent: Vec<u8> = vec![0x01, 0x00, 0x01];
    let modulus_bits = 1024;

    // pub key template
    let pub_key_template = vec![
        Attribute::Token(true),
        Attribute::Private(false),
        Attribute::PublicExponent(public_exponent),
        Attribute::ModulusBits(modulus_bits.into()),
    ];

    // priv key template
    let priv_key_template = vec![Attribute::Token(true)];

    // generate a key pair
    let (public, private) =
        session.generate_key_pair(&mechanism, &pub_key_template, &priv_key_template)?;

    // data to sign
    let data = [0xFF, 0x55, 0xDD];

    // sign something with it
    let signature = session.sign(&Mechanism::RsaPkcs, private, &data)?;

    // verify the signature
    session.verify(&Mechanism::RsaPkcs, public, &data, &signature)?;

    // delete keys
    session.destroy_object(public)?;
    session.destroy_object(private)?;

    Ok(())
}

#[test]
#[serial]
fn sign_verify_ed25519() -> TestResult {
    let (pkcs11, slot) = init_pins();

    let session = pkcs11.open_rw_session(slot)?;

    session.login(UserType::User, Some(&AuthPin::new(USER_PIN.into())))?;

    let mechanism = Mechanism::EccEdwardsKeyPairGen;

    let pub_key_template = vec![
        Attribute::Token(true),
        Attribute::Private(false),
        Attribute::Verify(true),
        // Ed25519 OID
        // See: https://github.com/opendnssec/SoftHSMv2/blob/ac70dc398b236e4522101930e790008936489e2d/src/lib/test/SignVerifyTests.cpp#L173
        Attribute::EcParams(vec![
            0x13, 0x0c, 0x65, 0x64, 0x77, 0x61, 0x72, 0x64, 0x73, 0x32, 0x35, 0x35, 0x31, 0x39,
        ]),
    ];

    let priv_key_template = vec![Attribute::Token(true)];

    let (public, private) =
        session.generate_key_pair(&mechanism, &pub_key_template, &priv_key_template)?;

    let data = [0xFF, 0x55, 0xDD];

    let signature = session.sign(&Mechanism::Eddsa, private, &data)?;

    session.verify(&Mechanism::Eddsa, public, &data, &signature)?;

    session.destroy_object(public)?;
    session.destroy_object(private)?;

    Ok(())
}

#[test]
#[serial]
fn encrypt_decrypt() -> TestResult {
    let (pkcs11, slot) = init_pins();

    // open a session
    let session = pkcs11.open_rw_session(slot)?;

    // log in the session
    session.login(UserType::User, Some(&AuthPin::new(USER_PIN.into())))?;

    // get mechanism
    let mechanism = Mechanism::RsaPkcsKeyPairGen;

    let public_exponent: Vec<u8> = vec![0x01, 0x00, 0x01];
    let modulus_bits = 1024;

    // pub key template
    let pub_key_template = vec![
        Attribute::Token(true),
        Attribute::Private(false),
        Attribute::PublicExponent(public_exponent),
        Attribute::ModulusBits(modulus_bits.into()),
        Attribute::Encrypt(true),
    ];

    // priv key template
    let priv_key_template = vec![Attribute::Token(true), Attribute::Decrypt(true)];

    // generate a key pair
    let (public, private) =
        session.generate_key_pair(&mechanism, &pub_key_template, &priv_key_template)?;

    // data to encrypt
    let data = vec![0xFF, 0x55, 0xDD];

    // encrypt something with it
    let encrypted_data = session.encrypt(&Mechanism::RsaPkcs, public, &data)?;

    // decrypt
    let decrypted_data = session.decrypt(&Mechanism::RsaPkcs, private, &encrypted_data)?;

    // The decrypted buffer is bigger than the original one.
    assert_eq!(data, decrypted_data);

    // delete keys
    session.destroy_object(public)?;
    session.destroy_object(private)?;

    Ok(())
}

#[test]
#[serial]
fn derive_key() -> TestResult {
    let (pkcs11, slot) = init_pins();

    // open a session
    let session = pkcs11.open_rw_session(slot)?;

    // log in the session
    session.login(UserType::User, Some(&AuthPin::new(USER_PIN.into())))?;

    // get mechanism
    let mechanism = Mechanism::EccKeyPairGen;

    let secp256r1_oid: Vec<u8> = vec![0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];

    // pub key template
    let pub_key_template = vec![
        Attribute::Token(true),
        Attribute::Private(false),
        Attribute::Derive(true),
        Attribute::KeyType(KeyType::EC),
        Attribute::Verify(true),
        Attribute::EcParams(secp256r1_oid),
    ];

    // priv key template
    let priv_key_template = vec![
        Attribute::Token(true),
        Attribute::Private(true),
        Attribute::Sensitive(true),
        Attribute::Extractable(false),
        Attribute::Derive(true),
        Attribute::Sign(true),
    ];

    // generate a key pair
    let (public, private) =
        session.generate_key_pair(&mechanism, &pub_key_template, &priv_key_template)?;

    let ec_point_attribute = session
        .get_attributes(public, &[AttributeType::EcPoint])?
        .remove(0);

    let ec_point = if let Attribute::EcPoint(point) = ec_point_attribute {
        point
    } else {
        panic!("Expected EC point attribute.");
    };

    use cryptoki::mechanism::elliptic_curve::*;

    let params = Ecdh1DeriveParams::new(EcKdf::null(), &ec_point);

    let shared_secret = session.derive_key(
        &Mechanism::Ecdh1Derive(params),
        private,
        &[
            Attribute::Class(ObjectClass::SECRET_KEY),
            Attribute::KeyType(KeyType::GENERIC_SECRET),
            Attribute::Sensitive(false),
            Attribute::Extractable(true),
            Attribute::Token(false),
        ],
    )?;

    let value_attribute = session
        .get_attributes(shared_secret, &[AttributeType::Value])?
        .remove(0);
    let value = if let Attribute::Value(value) = value_attribute {
        value
    } else {
        panic!("Expected value attribute.");
    };

    assert_eq!(value.len(), 32);

    // delete keys
    session.destroy_object(public)?;
    session.destroy_object(private)?;

    Ok(())
}

#[test]
#[serial]
fn import_export() -> TestResult {
    let (pkcs11, slot) = init_pins();

    // open a session
    let session = pkcs11.open_rw_session(slot)?;

    // log in the session
    session.login(UserType::User, Some(&AuthPin::new(USER_PIN.into())))?;

    let public_exponent: Vec<u8> = vec![0x01, 0x00, 0x01];
    let modulus = vec![0xFF; 1024];

    let template = vec![
        Attribute::Token(true),
        Attribute::Private(false),
        Attribute::PublicExponent(public_exponent),
        Attribute::Modulus(modulus.clone()),
        Attribute::Class(ObjectClass::PUBLIC_KEY),
        Attribute::KeyType(KeyType::RSA),
        Attribute::Verify(true),
    ];

    {
        // Intentionally forget the object handle to find it later
        let _public_key = session.create_object(&template)?;
    }

    let is_it_the_public_key = session.find_objects(&template)?.remove(0);

    let attribute_info = session
        .get_attribute_info(is_it_the_public_key, &[AttributeType::Modulus])?
        .remove(0);

    if let AttributeInfo::Available(size) = attribute_info {
        assert_eq!(size, 1024);
    } else {
        panic!("The Modulus attribute was expected to be present.")
    };

    let attr = session
        .get_attributes(is_it_the_public_key, &[AttributeType::Modulus])?
        .remove(0);

    if let Attribute::Modulus(modulus_cmp) = attr {
        assert_eq!(modulus[..], modulus_cmp[..]);
    } else {
        panic!("Expected the Modulus attribute.");
    }

    // delete key
    session.destroy_object(is_it_the_public_key)?;

    Ok(())
}

#[test]
#[serial]
fn get_token_info() -> TestResult {
    let (pkcs11, slot) = init_pins();
    let info = pkcs11.get_token_info(slot)?;
    assert_eq!("SoftHSM project", info.manufacturer_id());

    Ok(())
}

#[test]
#[serial]
fn wrap_and_unwrap_key() {
    let (pkcs11, slot) = init_pins();
    // open a session
    let session = pkcs11.open_rw_session(slot).unwrap();

    // log in the session
    session
        .login(UserType::User, Some(&AuthPin::new(USER_PIN.into())))
        .unwrap();

    let key_to_be_wrapped_template = vec![
        Attribute::Token(true),
        // the key needs to be extractable to be suitable for being wrapped
        Attribute::Extractable(true),
        Attribute::Encrypt(true),
    ];

    // generate a secret key that will be wrapped
    let key_to_be_wrapped = session
        .generate_key(&Mechanism::Des3KeyGen, &key_to_be_wrapped_template)
        .unwrap();

    // Des3Ecb input length must be a multiple of 8
    // see: PKCS#11 spec Table 10-10, DES-ECB Key And Data Length Constraints
    let encrypted_with_original = session
        .encrypt(
            &Mechanism::Des3Ecb,
            key_to_be_wrapped,
            &[1, 2, 3, 4, 5, 6, 7, 8],
        )
        .unwrap();

    // pub key template
    let pub_key_template = vec![
        Attribute::Token(true),
        Attribute::Private(true),
        Attribute::PublicExponent(vec![0x01, 0x00, 0x01]),
        Attribute::ModulusBits(1024.into()),
        // key needs to have "wrap" attribute to wrap other keys
        Attribute::Wrap(true),
    ];

    // priv key template
    let priv_key_template = vec![Attribute::Token(true)];

    let (wrapping_key, unwrapping_key) = session
        .generate_key_pair(
            &Mechanism::RsaPkcsKeyPairGen,
            &pub_key_template,
            &priv_key_template,
        )
        .unwrap();

    let wrapped_key = session
        .wrap_key(&Mechanism::RsaPkcs, wrapping_key, key_to_be_wrapped)
        .unwrap();
    assert_eq!(wrapped_key.len(), 128);

    let unwrapped_key = session
        .unwrap_key(
            &Mechanism::RsaPkcs,
            unwrapping_key,
            &wrapped_key,
            &[
                Attribute::Token(true),
                Attribute::Private(true),
                Attribute::Encrypt(true),
                Attribute::Class(ObjectClass::SECRET_KEY),
                Attribute::KeyType(KeyType::DES3),
            ],
        )
        .unwrap();

    let encrypted_with_unwrapped = session
        .encrypt(
            &Mechanism::Des3Ecb,
            unwrapped_key,
            &[1, 2, 3, 4, 5, 6, 7, 8],
        )
        .unwrap();
    assert_eq!(encrypted_with_original, encrypted_with_unwrapped);
}

#[test]
#[serial]
fn login_feast() {
    const SESSIONS: usize = 100;

    let (pkcs11, slot) = init_pins();
    let mut threads = Vec::new();

    for _ in 0..SESSIONS {
        let pkcs11 = pkcs11.clone();
        threads.push(thread::spawn(move || {
            let session = pkcs11.open_rw_session(slot).unwrap();
            match session.login(UserType::User, Some(&AuthPin::new(USER_PIN.into()))) {
                Ok(_) | Err(Error::Pkcs11(RvError::UserAlreadyLoggedIn)) => {}
                Err(e) => panic!("Bad error response: {}", e),
            }
            match session.login(UserType::User, Some(&AuthPin::new(USER_PIN.into()))) {
                Ok(_) | Err(Error::Pkcs11(RvError::UserAlreadyLoggedIn)) => {}
                Err(e) => panic!("Bad error response: {}", e),
            }
            match session.login(UserType::User, Some(&AuthPin::new(USER_PIN.into()))) {
                Ok(_) | Err(Error::Pkcs11(RvError::UserAlreadyLoggedIn)) => {}
                Err(e) => panic!("Bad error response: {}", e),
            }
            match session.logout() {
                Ok(_) | Err(Error::Pkcs11(RvError::UserNotLoggedIn)) => {}
                Err(e) => panic!("Bad error response: {}", e),
            }
            match session.logout() {
                Ok(_) | Err(Error::Pkcs11(RvError::UserNotLoggedIn)) => {}
                Err(e) => panic!("Bad error response: {}", e),
            }
            match session.logout() {
                Ok(_) | Err(Error::Pkcs11(RvError::UserNotLoggedIn)) => {}
                Err(e) => panic!("Bad error response: {}", e),
            }
        }));
    }

    for thread in threads {
        thread.join().unwrap();
    }
}

#[test]
#[serial]
fn get_info_test() -> TestResult {
    let (pkcs11, _) = init_pins();
    let info = pkcs11.get_library_info()?;

    assert_eq!(info.cryptoki_version().major(), 2);
    assert_eq!(info.cryptoki_version().minor(), 40);
    assert_eq!(info.manufacturer_id(), String::from("SoftHSM"));
    Ok(())
}

#[test]
#[serial]
fn get_slot_info_test() -> TestResult {
    let (pkcs11, slot) = init_pins();
    let slot_info = pkcs11.get_slot_info(slot)?;
    assert!(slot_info.token_present());
    assert!(!slot_info.hardware_slot());
    assert!(!slot_info.removable_device());
    assert_eq!(slot_info.manufacturer_id(), String::from("SoftHSM project"));
    Ok(())
}

#[test]
#[serial]
fn get_session_info_test() -> TestResult {
    let (pkcs11, slot) = init_pins();
    {
        let session = pkcs11.open_ro_session(slot)?;
        let session_info = session.get_session_info()?;
        assert!(!session_info.read_write());
        assert_eq!(session_info.slot_id(), slot);
        assert!(matches!(
            session_info.session_state(),
            SessionState::RoPublic
        ));

        session.login(UserType::User, Some(&AuthPin::new(USER_PIN.into())))?;
        let session_info = session.get_session_info()?;
        assert!(!session_info.read_write());
        assert_eq!(session_info.slot_id(), slot);
        assert!(matches!(session_info.session_state(), SessionState::RoUser));
        session.logout()?;
        if let Err(cryptoki::error::Error::Pkcs11(rv_error)) =
            session.login(UserType::So, Some(&AuthPin::new(SO_PIN.into())))
        {
            assert_eq!(rv_error, RvError::SessionReadOnlyExists)
        } else {
            panic!("Should error when attempting to log in as CKU_SO on a read-only session");
        }
    }

    let session = pkcs11.open_rw_session(slot)?;
    let session_info = session.get_session_info()?;
    assert!(session_info.read_write());
    assert_eq!(session_info.slot_id(), slot);
    assert!(matches!(
        session_info.session_state(),
        SessionState::RwPublic
    ));

    session.login(UserType::User, Some(&AuthPin::new(USER_PIN.into())))?;
    let session_info = session.get_session_info()?;
    assert!(session_info.read_write());
    assert_eq!(session_info.slot_id(), slot);
    assert!(matches!(session_info.session_state(), SessionState::RwUser,));
    session.logout()?;
    session.login(UserType::So, Some(&AuthPin::new(SO_PIN.into())))?;
    let session_info = session.get_session_info()?;
    assert!(session_info.read_write());
    assert_eq!(session_info.slot_id(), slot);
    assert!(matches!(
        session_info.session_state(),
        SessionState::RwSecurityOfficer
    ));

    Ok(())
}

#[test]
#[serial]
fn generate_random_test() -> TestResult {
    let (pkcs11, slot) = init_pins();

    let session = pkcs11.open_ro_session(slot)?;

    let poor_seed: [u8; 32] = [0; 32];
    session.seed_random(&poor_seed)?;

    let mut random_data: [u8; 32] = [0; 32];
    session.generate_random_slice(&mut random_data)?;

    // This of course assumes the RBG in the the SoftHSM is not terrible
    assert!(!random_data.iter().all(|&x| x == 0));

    let random_vec = session.generate_random_vec(32)?;
    assert_eq!(random_vec.len(), 32);

    assert!(!random_vec.iter().all(|&x| x == 0));
    Ok(())
}

#[test]
#[serial]
fn set_pin_test() -> TestResult {
    let new_user_pin = "123456";
    let (pkcs11, slot) = init_pins();

    let session = pkcs11.open_rw_session(slot)?;
    let user_pin = AuthPin::new(USER_PIN.into());
    let new_user_pin = AuthPin::new(new_user_pin.into());

    session.login(UserType::User, Some(&user_pin))?;
    session.set_pin(&user_pin, &new_user_pin)?;
    session.logout()?;
    session.login(UserType::User, Some(&new_user_pin))?;

    Ok(())
}

#[test]
#[serial]
fn get_attribute_info_test() -> TestResult {
    let (pkcs11, slot) = init_pins();

    // open a session
    let session = pkcs11.open_rw_session(slot)?;

    // log in the session
    session.login(UserType::User, Some(&AuthPin::new(USER_PIN.into())))?;

    // get mechanism
    let mechanism = Mechanism::RsaPkcsKeyPairGen;

    let public_exponent: Vec<u8> = vec![0x01, 0x00, 0x01];
    let modulus_bits = 2048;

    // pub key template
    let pub_key_template = vec![
        Attribute::Token(false),
        Attribute::Private(false),
        Attribute::PublicExponent(public_exponent),
        Attribute::ModulusBits(modulus_bits.into()),
    ];

    // priv key template
    let priv_key_template = vec![
        Attribute::Token(false),
        Attribute::Sensitive(true),
        Attribute::Extractable(false),
    ];

    // generate a key pair
    let (public, private) =
        session.generate_key_pair(&mechanism, &pub_key_template, &priv_key_template)?;

    let pub_attribs = vec![AttributeType::PublicExponent, AttributeType::Modulus];
    let mut priv_attribs = pub_attribs.clone();
    priv_attribs.push(AttributeType::PrivateExponent);

    let attrib_info = session.get_attribute_info(public, &pub_attribs)?;
    let hash = pub_attribs
        .iter()
        .zip(attrib_info.iter())
        .collect::<HashMap<_, _>>();

    if let AttributeInfo::Available(size) = hash[&AttributeType::Modulus] {
        assert_eq!(*size, 2048 / 8);
    } else {
        panic!("Modulus should not return Unavailable for an RSA public key");
    }

    match hash[&AttributeType::PublicExponent] {
        AttributeInfo::Available(_) => {}
        _ => panic!("Public Exponent should not return Unavailable for an RSA public key"),
    }

    let attrib_info = session.get_attribute_info(private, &priv_attribs)?;
    let hash = priv_attribs
        .iter()
        .zip(attrib_info.iter())
        .collect::<HashMap<_, _>>();

    if let AttributeInfo::Available(size) = hash[&AttributeType::Modulus] {
        assert_eq!(*size, 2048 / 8);
    } else {
        panic!("Modulus should not return Unavailable on an RSA private key");
    }

    match hash[&AttributeType::PublicExponent] {
        AttributeInfo::Available(_) => {}
        _ => panic!("PublicExponent should not return Unavailable on an RSA private key"),
    }

    match hash[&AttributeType::PrivateExponent] {
        AttributeInfo::Sensitive => {}
        _ => panic!("Private Exponent of RSA private key should be sensitive"),
    }

    let hash = session.get_attribute_info_map(private, priv_attribs)?;
    if let AttributeInfo::Available(size) = hash[&AttributeType::Modulus] {
        assert_eq!(size, 2048 / 8);
    } else {
        panic!("Modulus should not return Unavailable on an RSA private key");
    }

    match hash[&AttributeType::PublicExponent] {
        AttributeInfo::Available(_) => {}
        _ => panic!("Public Exponent should not return Unavailable for an RSA private key"),
    }

    match hash[&AttributeType::PrivateExponent] {
        AttributeInfo::Sensitive => {}
        _ => panic!("Private Exponent of RSA private key should be sensitive"),
    }

    Ok(())
}

#[test]
#[serial]
fn is_fn_supported_test() {
    use cryptoki::context::Function;

    let (pkcs11, _) = init_pins();

    assert!(
        pkcs11.is_fn_supported(Function::Initialize),
        "C_Initialize function reports as not supported"
    );
    assert!(
        pkcs11.is_fn_supported(Function::Sign),
        "C_Sign function reports as not supported"
    );
    assert!(
        pkcs11.is_fn_supported(Function::DigestFinal),
        "C_DigestFinal function reports as not supported"
    );
}

#[test]
#[serial]
fn is_initialized_test() {
    use cryptoki::context::{CInitializeArgs, Pkcs11};

    let mut pkcs11 = Pkcs11::new(
        std::env::var("PKCS11_SOFTHSM2_MODULE")
            .unwrap_or_else(|_| "/usr/local/lib/softhsm/libsofthsm2.so".to_string()),
    )
    .unwrap();

    assert!(
        !pkcs11.is_initialized(),
        "Context created with initialized flag on"
    );

    // initialize the library
    pkcs11.initialize(CInitializeArgs::OsThreads).unwrap();

    assert!(
        pkcs11.is_initialized(),
        "Context was not marked as initialized"
    );

    match pkcs11.initialize(CInitializeArgs::OsThreads) {
        Err(Error::AlreadyInitialized) => (),
        Err(e) => panic!("Got unexpected error when initializing: {}", e),
        Ok(()) => panic!("Initializing twice should not have been allowed"),
    }
}

#[test]
#[serial]
fn aes_key_attributes_test() -> TestResult {
    let (pkcs11, slot) = init_pins();

    // open a session
    let session = pkcs11.open_rw_session(slot)?;

    // log in the session
    session.login(UserType::User, Some(&AuthPin::new(USER_PIN.into())))?;

    // get mechanism
    let mechanism = Mechanism::AesKeyGen;

    // pub key template
    let key_template = vec![
        Attribute::Class(ObjectClass::SECRET_KEY),
        Attribute::Token(true),
        Attribute::Sensitive(true),
        Attribute::ValueLen(16.into()),
        Attribute::KeyType(KeyType::AES),
        Attribute::Label(b"testAES".to_vec()),
        Attribute::Private(true),
    ];

    // generate a key pair
    let key = session.generate_key(&mechanism, &key_template)?;

    let mut attributes_result =
        session.get_attributes(key, &[AttributeType::EndDate, AttributeType::StartDate])?;

    if let Some(Attribute::StartDate(date)) = attributes_result.pop() {
        assert!(date.is_empty());
    } else {
        panic!("Last attribute was not a start date");
    }

    if let Some(Attribute::EndDate(date)) = attributes_result.pop() {
        assert!(date.is_empty());
    } else {
        panic!("First attribute was not an end date");
    }

    Ok(())
}

#[test]
#[serial]
fn ro_rw_session_test() -> TestResult {
    let public_exponent: Vec<u8> = vec![0x01, 0x00, 0x01];
    let modulus = vec![0xFF; 1024];

    let template = vec![
        Attribute::Token(true),
        Attribute::Private(false),
        Attribute::PublicExponent(public_exponent),
        Attribute::Modulus(modulus),
        Attribute::Class(ObjectClass::PUBLIC_KEY),
        Attribute::KeyType(KeyType::RSA),
        Attribute::Verify(true),
    ];

    let (pkcs11, slot) = init_pins();

    // Try out Read-Only session
    {
        // open a session
        let ro_session = pkcs11.open_ro_session(slot)?;

        // log in the session
        ro_session.login(UserType::User, Some(&AuthPin::new(USER_PIN.into())))?;

        // generate a key pair
        // This should NOT work using the Read-Only session
        let e = ro_session.create_object(&template).unwrap_err();

        if let Error::Pkcs11(RvError::SessionReadOnly) = e {
            // as expected
        } else {
            panic!("Got wrong error code (expecting SessionReadOnly): {}", e);
        }
        ro_session.logout()?;
    }

    // Try out Read/Write session
    {
        // open a session
        let rw_session = pkcs11.open_rw_session(slot)?;

        // log in the session
        rw_session.login(UserType::User, Some(&AuthPin::new(USER_PIN.into())))?;

        // generate a key pair
        // This should work using the Read/Write session
        let object = rw_session.create_object(&template)?;

        // delete keys
        rw_session.destroy_object(object)?;
        rw_session.logout()?;
    }

    Ok(())
}

#[test]
#[serial]
fn aes_cbc_encrypt() -> TestResult {
    // Encrypt two blocks of zeros with AES-128-CBC, and zero IV
    let key = vec![0; 16];
    let iv = [0; 16];
    let plain = [0; 32];
    let expected_cipher = [
        0x66, 0xe9, 0x4b, 0xd4, 0xef, 0x8a, 0x2c, 0x3b, 0x88, 0x4c, 0xfa, 0x59, 0xca, 0x34, 0x2b,
        0x2e, 0xf7, 0x95, 0xbd, 0x4a, 0x52, 0xe2, 0x9e, 0xd7, 0x13, 0xd3, 0x13, 0xfa, 0x20, 0xe9,
        0x8d, 0xbc,
    ];

    let (pkcs11, slot) = init_pins();
    let session = pkcs11.open_rw_session(slot)?;
    session.login(UserType::User, Some(&AuthPin::new(USER_PIN.into())))?;

    let template = [
        Attribute::Class(ObjectClass::SECRET_KEY),
        Attribute::KeyType(KeyType::AES),
        Attribute::Value(key),
        Attribute::Encrypt(true),
    ];
    let key_handle = session.create_object(&template)?;
    let mechanism = Mechanism::AesCbc(iv);
    let cipher = session.encrypt(&mechanism, key_handle, &plain)?;
    assert_eq!(expected_cipher[..], cipher[..]);
    Ok(())
}

#[test]
#[serial]
fn aes_cbc_pad_encrypt() -> TestResult {
    // Encrypt two blocks of zeros with AES-128-CBC and PKCS#7 padding, and zero IV
    let key = vec![0; 16];
    let iv = [0; 16];
    let plain = [0; 32];
    let expected_cipher = [
        0x66, 0xe9, 0x4b, 0xd4, 0xef, 0x8a, 0x2c, 0x3b, 0x88, 0x4c, 0xfa, 0x59, 0xca, 0x34, 0x2b,
        0x2e, 0xf7, 0x95, 0xbd, 0x4a, 0x52, 0xe2, 0x9e, 0xd7, 0x13, 0xd3, 0x13, 0xfa, 0x20, 0xe9,
        0x8d, 0xbc, 0x5c, 0x04, 0x76, 0x16, 0x75, 0x6f, 0xdc, 0x1c, 0x32, 0xe0, 0xdf, 0x6e, 0x8c,
        0x59, 0xbb, 0x2a,
    ];

    let (pkcs11, slot) = init_pins();
    let session = pkcs11.open_rw_session(slot)?;
    session.login(UserType::User, Some(&AuthPin::new(USER_PIN.into())))?;

    let template = [
        Attribute::Class(ObjectClass::SECRET_KEY),
        Attribute::KeyType(KeyType::AES),
        Attribute::Value(key),
        Attribute::Encrypt(true),
    ];
    let key_handle = session.create_object(&template)?;
    let mechanism = Mechanism::AesCbcPad(iv);
    let cipher = session.encrypt(&mechanism, key_handle, &plain)?;
    assert_eq!(expected_cipher[..], cipher[..]);
    Ok(())
}

#[test]
#[serial]
fn update_attributes_key() -> TestResult {
    let (pkcs11, slot) = init_pins();
    // open a session
    let session = pkcs11.open_rw_session(slot)?;

    // log in the session
    session.login(UserType::User, Some(&AuthPin::new(USER_PIN.into())))?;

    // pub key template
    let pub_key_template = vec![
        Attribute::Token(true),
        Attribute::Private(true),
        Attribute::PublicExponent(vec![0x01, 0x00, 0x01]),
        Attribute::ModulusBits(1024.into()),
    ];

    // priv key template
    let priv_key_template = vec![Attribute::Token(true), Attribute::Extractable(true)];

    let (_public_key, private_key) = session.generate_key_pair(
        &Mechanism::RsaPkcsKeyPairGen,
        &pub_key_template,
        &priv_key_template,
    )?;

    let updated_attributes = vec![Attribute::Extractable(false)];

    session.update_attributes(private_key, &updated_attributes)?;

    let mut attributes_result =
        session.get_attributes(private_key, &[AttributeType::Extractable])?;

    if let Some(Attribute::Extractable(ext)) = attributes_result.pop() {
        assert!(!ext);
    } else {
        panic!("Last attribute was not extractable");
    }

    Ok(())
}

#[test]
#[serial]
fn sha256_digest() -> TestResult {
    let (pkcs11, slot) = init_pins();

    // open a session
    let session = pkcs11.open_rw_session(slot)?;

    // log in the session
    session.login(UserType::User, Some(&AuthPin::new(USER_PIN.into())))?;

    // data to digest
    let data = vec![0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];

    let want = vec![
        0x17, 0x22, 0x6b, 0x1f, 0x68, 0xae, 0xba, 0xcd, 0xef, 0x07, 0x46, 0x45, 0x0f, 0x64, 0x28,
        0x74, 0x63, 0x8b, 0x29, 0x57, 0x07, 0xef, 0x73, 0xfb, 0x2c, 0x6b, 0xb7, 0xf8, 0x8e, 0x89,
        0x92, 0x9f,
    ];
    let have = session.digest(&Mechanism::Sha256, &data)?;
    assert_eq!(want[..], have[..]);

    Ok(())
}

#[test]
#[serial]
// Currently empty AAD crashes SoftHSM, see: https://github.com/opendnssec/SoftHSMv2/issues/605
#[ignore]
fn aes_gcm_no_aad() -> TestResult {
    // Encrypt two blocks of zeros with AES-128-GCM
    let key = vec![0; 16];
    let iv = [0; 12];
    let aad = [];
    let plain = [0; 32];
    let expected_cipher_and_tag = [
        0x03, 0x88, 0xda, 0xce, 0x60, 0xb6, 0xa3, 0x92, 0xf3, 0x28, 0xc2, 0xb9, 0x71, 0xb2, 0xfe,
        0x78, 0xf7, 0x95, 0xaa, 0xab, 0x49, 0x4b, 0x59, 0x23, 0xf7, 0xfd, 0x89, 0xff, 0x94, 0x8b,
        0xc1, 0xe0, 0x40, 0x49, 0x0a, 0xf4, 0x80, 0x56, 0x06, 0xb2, 0xa3, 0xa2, 0xe7, 0x93,
    ];

    let (pkcs11, slot) = init_pins();
    let session = pkcs11.open_rw_session(slot)?;
    session.login(UserType::User, Some(&AuthPin::new(USER_PIN.into())))?;

    let template = [
        Attribute::Class(ObjectClass::SECRET_KEY),
        Attribute::KeyType(KeyType::AES),
        Attribute::Value(key),
        Attribute::Encrypt(true),
    ];
    let key_handle = session.create_object(&template)?;
    let mechanism = Mechanism::AesGcm(GcmParams::new(&iv, &aad, 96.into()));
    let cipher_and_tag = session.encrypt(&mechanism, key_handle, &plain)?;
    assert_eq!(expected_cipher_and_tag[..], cipher_and_tag[..]);
    Ok(())
}

#[test]
#[serial]
fn aes_gcm_with_aad() -> TestResult {
    // Encrypt a block of zeros with AES-128-GCM.
    // Use another block of zeros for AAD.
    let key = vec![0; 16];
    let iv = [0; 12];
    let aad = [0; 16];
    let plain = [0; 16];
    let expected_cipher_and_tag = [
        0x03, 0x88, 0xda, 0xce, 0x60, 0xb6, 0xa3, 0x92, 0xf3, 0x28, 0xc2, 0xb9, 0x71, 0xb2, 0xfe,
        0x78, 0xd2, 0x4e, 0x50, 0x3a, 0x1b, 0xb0, 0x37, 0x07, 0x1c, 0x71, 0xb3, 0x5d,
    ];

    let (pkcs11, slot) = init_pins();
    let session = pkcs11.open_rw_session(slot)?;
    session.login(UserType::User, Some(&AuthPin::new(USER_PIN.into())))?;

    let template = [
        Attribute::Class(ObjectClass::SECRET_KEY),
        Attribute::KeyType(KeyType::AES),
        Attribute::Value(key),
        Attribute::Encrypt(true),
    ];
    let key_handle = session.create_object(&template)?;
    let mechanism = Mechanism::AesGcm(GcmParams::new(&iv, &aad, 96.into()));
    let cipher_and_tag = session.encrypt(&mechanism, key_handle, &plain)?;
    assert_eq!(expected_cipher_and_tag[..], cipher_and_tag[..]);
    Ok(())
}

#[test]
#[serial]
fn get_slot_event() -> TestResult {
    // Not implemented in SoftHSMv2
    // https://github.com/opendnssec/SoftHSMv2/issues/370
    let (pkcs11, _slot) = init_pins();
    let event = pkcs11.get_slot_event()?;
    assert_eq!(None, event);
    Ok(())
}

#[test]
#[serial]
fn wait_for_slot_event() {
    // Not implemented in SoftHSMv2
    // https://github.com/opendnssec/SoftHSMv2/issues/370
    let (pkcs11, _slot) = init_pins();
    let res = pkcs11.wait_for_slot_event();

    assert!(
        matches!(res, Err(Error::Pkcs11(RvError::FunctionNotSupported))),
        "res = {:?}",
        res
    );
}
