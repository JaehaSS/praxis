//! `.parquet` 파일 미리보기.
//!
//! 에디터가 파케이 파일을 표 형태로 보여줄 수 있도록, 스키마(첫 [`PREVIEW_COLUMNS`]개
//! 루트 필드)와 처음 [`PREVIEW_ROWS`]행을 읽어 JSON 문자열로 직렬화한다.
//!
//! 파케이 파일은 흔히 [`super::MAX_FILE_BYTES`](2MiB)를 넘는다 — 그래서 `read_file`은
//! 이 경로를 크기 검사 **이전에** 분기한다. 대신 이 모듈이 직접 미리보기 크기를 제한한다
//! (행/컬럼 개수 상한만, 파일 자체는 전체를 스트리밍으로 읽는다).
//!
//! 절대 panic하지 않는다 — 어떤 실패든 반환되는 JSON의 `error` 필드에 담긴다(프런트가
//! 파싱 실패를 걱정할 필요가 없도록).

use std::fs::File;
use std::path::Path;

use parquet::basic::{ConvertedType, LogicalType, TimeUnit, Type as PhysicalType};
use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::record::reader::RowIter;
use parquet::record::Field;
use parquet::schema::types::Type;
use serde::{Deserialize, Serialize};

/// 미리보기에 포함할 최대 행 수.
pub const PREVIEW_ROWS: usize = 200;
/// 미리보기에 포함할 최대 컬럼 수(스키마 루트 필드 기준).
pub const PREVIEW_COLUMNS: usize = 100;

/// 미리보기 컬럼 하나 — 이름과 사람이 읽을 수 있는 타입 라벨.
#[derive(Debug, Serialize, Deserialize)]
pub struct ColumnPreview {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
}

/// `parquet_preview`가 반환하는 JSON의 형태.
#[derive(Debug, Serialize, Deserialize)]
pub struct TablePreview {
    pub format: String,
    pub columns: Vec<ColumnPreview>,
    pub rows: Vec<Vec<Option<String>>>,
    pub total_rows: i64,
    pub total_columns: usize,
    pub row_groups: usize,
    pub shown_rows: usize,
    pub shown_columns: usize,
    pub error: Option<String>,
}

impl TablePreview {
    /// 아직 스키마도 못 읽은 상태의 실패 — 모든 카운트가 0.
    fn failed(message: impl Into<String>) -> Self {
        Self {
            format: "parquet".to_string(),
            columns: Vec::new(),
            rows: Vec::new(),
            total_rows: 0,
            total_columns: 0,
            row_groups: 0,
            shown_rows: 0,
            shown_columns: 0,
            error: Some(message.into()),
        }
    }
}

/// `.parquet` 파일을 열어 미리보기를 만들고 JSON 문자열로 직렬화한다.
///
/// 열기·파싱·행 순회 중 어떤 단계에서 실패해도 panic/err 하지 않는다 — 그 시점까지
/// 읽은 것(스키마 등)은 보존하고 `error` 필드에 사유를 담아 반환한다.
pub fn parquet_preview(path: &Path) -> String {
    let preview = build_preview(path);
    serde_json::to_string(&preview).unwrap_or_else(|e| {
        // TablePreview 직렬화가 실패할 길은 사실상 없지만(전 필드가 순수 데이터),
        // "절대 panic/err 하지 않는다" 계약을 지키기 위해 손으로 최소 JSON을 만든다.
        format!(
            r#"{{"format":"parquet","columns":[],"rows":[],"total_rows":0,"total_columns":0,"row_groups":0,"shown_rows":0,"shown_columns":0,"error":"직렬화 실패: {e}"}}"#
        )
    })
}

fn build_preview(path: &Path) -> TablePreview {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => return TablePreview::failed(format!("파일을 열 수 없습니다: {e}")),
    };
    let reader = match SerializedFileReader::new(file) {
        Ok(r) => r,
        Err(e) => return TablePreview::failed(format!("parquet 파일을 파싱할 수 없습니다: {e}")),
    };

    let metadata = reader.metadata();
    let total_rows = metadata.file_metadata().num_rows();
    let row_groups = metadata.num_row_groups();
    let root = metadata.file_metadata().schema();
    let root_fields = root.get_fields();
    let total_columns = root_fields.len();
    let take_n = total_columns.min(PREVIEW_COLUMNS);
    let projected_fields = root_fields[..take_n].to_vec();

    let columns: Vec<ColumnPreview> = projected_fields
        .iter()
        .map(|f| ColumnPreview {
            name: f.name().to_string(),
            type_: type_label(f),
        })
        .collect();
    let shown_columns = columns.len();

    if take_n == 0 {
        // 컬럼이 없으면(빈 스키마) 프로젝션을 만들 것도, 행을 읽을 것도 없다.
        return TablePreview {
            format: "parquet".to_string(),
            columns,
            rows: Vec::new(),
            total_rows,
            total_columns,
            row_groups,
            shown_rows: 0,
            shown_columns,
            error: None,
        };
    }

    let projection = match Type::group_type_builder(root.name())
        .with_fields(projected_fields)
        .build()
    {
        Ok(t) => t,
        Err(e) => {
            return TablePreview {
                format: "parquet".to_string(),
                columns,
                rows: Vec::new(),
                total_rows,
                total_columns,
                row_groups,
                shown_rows: 0,
                shown_columns,
                error: Some(format!("프로젝션 스키마 생성 실패: {e}")),
            };
        }
    };

    let row_iter = match RowIter::from_file(Some(projection), &reader) {
        Ok(it) => it,
        Err(e) => {
            return TablePreview {
                format: "parquet".to_string(),
                columns,
                rows: Vec::new(),
                total_rows,
                total_columns,
                row_groups,
                shown_rows: 0,
                shown_columns,
                error: Some(format!("행 이터레이터 생성 실패: {e}")),
            };
        }
    };

    let mut rows = Vec::new();
    let mut error = None;
    for row_result in row_iter.take(PREVIEW_ROWS) {
        match row_result {
            Ok(row) => {
                let cells = row
                    .get_column_iter()
                    .map(|(_, field)| field_to_cell(field))
                    .collect();
                rows.push(cells);
            }
            Err(e) => {
                error = Some(format!("행을 읽는 중 오류: {e}"));
                break;
            }
        }
    }

    TablePreview {
        format: "parquet".to_string(),
        columns,
        shown_rows: rows.len(),
        rows,
        total_rows,
        total_columns,
        row_groups,
        shown_columns,
        error,
    }
}

/// 셀 값 하나를 문자열로 변환. `Null`은 JSON `null`, 문자열은 원문 그대로(Display의
/// 겹따옴표를 벗긴다), 바이트열은 길이만, 그 외는 `Field`의 `Display`(날짜·타임스탬프·
/// 소수·그룹·리스트를 이미 사람이 읽기 좋게 포맷한다)를 그대로 쓴다.
fn field_to_cell(field: &Field) -> Option<String> {
    match field {
        Field::Null => None,
        Field::Str(s) => Some(s.clone()),
        Field::Bytes(b) => Some(format!("<{} bytes>", b.len())),
        other => Some(other.to_string()),
    }
}

/// 필드(스키마 노드)의 사람이 읽는 타입 라벨.
fn type_label(field: &Type) -> String {
    let basic = field.get_basic_info();
    if let Some(lt) = basic.logical_type_ref() {
        return logical_type_label(lt);
    }
    match field {
        Type::GroupType { .. } => match basic.converted_type() {
            ConvertedType::LIST => "list".to_string(),
            ConvertedType::MAP | ConvertedType::MAP_KEY_VALUE => "map".to_string(),
            _ => "struct".to_string(),
        },
        Type::PrimitiveType {
            physical_type,
            type_length,
            scale,
            precision,
            ..
        } => match (basic.converted_type(), physical_type) {
            // 논리 타입 없이 converted type만 적힌 옛 파일(UTF8 등)도 같은 라벨을 받는다.
            (ConvertedType::UTF8, _) => "string".to_string(),
            (ConvertedType::DATE, _) => "date".to_string(),
            (ConvertedType::TIMESTAMP_MILLIS, _) => "timestamp[ms]".to_string(),
            (ConvertedType::TIMESTAMP_MICROS, _) => "timestamp[us]".to_string(),
            (ConvertedType::TIME_MILLIS, _) => "time[ms]".to_string(),
            (ConvertedType::TIME_MICROS, _) => "time[us]".to_string(),
            (ConvertedType::DECIMAL, _) => format!("decimal({precision},{scale})"),
            (ConvertedType::JSON, _) => "json".to_string(),
            (ConvertedType::ENUM, _) => "enum".to_string(),
            (ConvertedType::INT_8, _) => "int8".to_string(),
            (ConvertedType::INT_16, _) => "int16".to_string(),
            (ConvertedType::INT_32, _) => "int32".to_string(),
            (ConvertedType::INT_64, _) => "int64".to_string(),
            (ConvertedType::UINT_8, _) => "uint8".to_string(),
            (ConvertedType::UINT_16, _) => "uint16".to_string(),
            (ConvertedType::UINT_32, _) => "uint32".to_string(),
            (ConvertedType::UINT_64, _) => "uint64".to_string(),
            (_, PhysicalType::BOOLEAN) => "boolean".to_string(),
            (_, PhysicalType::INT32) => "int32".to_string(),
            (_, PhysicalType::INT64) => "int64".to_string(),
            (_, PhysicalType::INT96) => "int96".to_string(),
            (_, PhysicalType::FLOAT) => "float".to_string(),
            (_, PhysicalType::DOUBLE) => "double".to_string(),
            (_, PhysicalType::BYTE_ARRAY) => "byte_array".to_string(),
            (_, PhysicalType::FIXED_LEN_BYTE_ARRAY) => {
                format!("fixed_len_byte_array({type_length})")
            }
        },
    }
}

/// 논리 타입(있으면 물리 타입보다 우선)의 라벨.
fn logical_type_label(lt: &LogicalType) -> String {
    match lt {
        LogicalType::String => "string".to_string(),
        LogicalType::Date => "date".to_string(),
        LogicalType::Timestamp(t) => format!("timestamp[{}]", time_unit_label(&t.unit)),
        LogicalType::Time(t) => format!("time[{}]", time_unit_label(&t.unit)),
        LogicalType::Decimal(d) => format!("decimal({},{})", d.precision, d.scale),
        LogicalType::Integer(i) => {
            if i.is_signed {
                format!("int{}", i.bit_width)
            } else {
                format!("uint{}", i.bit_width)
            }
        }
        LogicalType::List => "list".to_string(),
        LogicalType::Map => "map".to_string(),
        LogicalType::Json => "json".to_string(),
        LogicalType::Uuid => "uuid".to_string(),
        LogicalType::Enum => "enum".to_string(),
        other => format!("{other:?}").to_lowercase(),
    }
}

fn time_unit_label(unit: &TimeUnit) -> &'static str {
    match unit {
        TimeUnit::MILLIS => "ms",
        TimeUnit::MICROS => "us",
        TimeUnit::NANOS => "ns",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parquet::data_type::{ByteArray, DoubleType, Int32Type, Int64Type};
    use parquet::file::properties::WriterProperties;
    use parquet::file::writer::SerializedFileWriter;
    use parquet::schema::parser::parse_message_type;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// 프로세스 내 유일한 임시 파일 경로(`fsapi::tests::tmp_root`와 같은 패턴).
    fn tmp_path(name: &str) -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = crate::testtmp::dir().join(format!(
            "praxis-fsapi-table-{}-{}",
            std::process::id(),
            n
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    /// id(int64, required) / name(utf8, optional, 2번째 행이 null) / value(double, required)
    /// 세 컬럼짜리 파일을 만든다.
    fn write_small_file(path: &Path) {
        let schema = Arc::new(
            parse_message_type(
                "message schema {
                    REQUIRED INT64 id;
                    OPTIONAL BYTE_ARRAY name (UTF8);
                    REQUIRED DOUBLE value;
                }",
            )
            .unwrap(),
        );
        let file = File::create(path).unwrap();
        let props = Arc::new(WriterProperties::builder().build());
        let mut writer = SerializedFileWriter::new(file, schema, props).unwrap();
        let mut rg = writer.next_row_group().unwrap();

        let mut id_writer = rg.next_column().unwrap().unwrap();
        id_writer
            .typed::<Int64Type>()
            .write_batch(&[1, 2], None, None)
            .unwrap();
        id_writer.close().unwrap();

        let mut name_writer = rg.next_column().unwrap().unwrap();
        name_writer
            .typed::<parquet::data_type::ByteArrayType>()
            .write_batch(
                &[ByteArray::from("abc".to_string().into_bytes())],
                Some(&[1, 0]),
                None,
            )
            .unwrap();
        name_writer.close().unwrap();

        let mut value_writer = rg.next_column().unwrap().unwrap();
        value_writer
            .typed::<DoubleType>()
            .write_batch(&[1.5, 2.5], None, None)
            .unwrap();
        value_writer.close().unwrap();

        rg.close().unwrap();
        writer.close().unwrap();
    }

    #[test]
    fn 작은_파일의_컬럼_타입_값_널을_그대로_돌려준다() {
        let path = tmp_path("small.parquet");
        write_small_file(&path);

        let json = parquet_preview(&path);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("유효한 JSON이어야 함");
        assert_eq!(parsed["error"], serde_json::Value::Null);
        assert_eq!(parsed["total_rows"], 2);
        assert_eq!(parsed["total_columns"], 3);
        assert_eq!(parsed["shown_rows"], 2);
        assert_eq!(parsed["shown_columns"], 3);
        assert_eq!(parsed["row_groups"], 1);
        assert_eq!(parsed["columns"][0]["name"], "id");
        assert_eq!(parsed["columns"][0]["type"], "int64");
        assert_eq!(parsed["columns"][1]["name"], "name");
        assert_eq!(parsed["columns"][1]["type"], "string");
        assert_eq!(parsed["columns"][2]["name"], "value");
        assert_eq!(parsed["columns"][2]["type"], "double");
        assert_eq!(parsed["rows"][0][0], "1");
        assert_eq!(parsed["rows"][0][1], "abc");
        assert_eq!(parsed["rows"][0][2], "1.5");
        assert_eq!(parsed["rows"][1][0], "2");
        assert_eq!(parsed["rows"][1][1], serde_json::Value::Null);
        assert_eq!(parsed["rows"][1][2], "2.5");

        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn read_file은_parquet를_table_kind로_반환한다() {
        let root = crate::testtmp::dir().join(format!(
            "praxis-fsapi-table-readfile-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("small.parquet");
        write_small_file(&path);

        let fc = super::super::read_file(&root, "small.parquet").unwrap();
        assert_eq!(fc.kind, super::super::FileKind::Table);
        let parsed: TablePreview = serde_json::from_str(&fc.content).unwrap();
        assert_eq!(parsed.total_rows, 2);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 행_250개면_200개만_보여주고_전체_개수는_유지한다() {
        let path = tmp_path("many_rows.parquet");
        let schema = Arc::new(
            parse_message_type("message schema { REQUIRED INT64 id; }").unwrap(),
        );
        let file = File::create(&path).unwrap();
        let props = Arc::new(WriterProperties::builder().build());
        let mut writer = SerializedFileWriter::new(file, schema, props).unwrap();
        let mut rg = writer.next_row_group().unwrap();
        let mut col = rg.next_column().unwrap().unwrap();
        let values: Vec<i64> = (0..250).collect();
        col.typed::<Int64Type>()
            .write_batch(&values, None, None)
            .unwrap();
        col.close().unwrap();
        rg.close().unwrap();
        writer.close().unwrap();

        let json = parquet_preview(&path);
        let parsed: TablePreview = serde_json::from_str(&json).unwrap();
        assert!(parsed.error.is_none());
        assert_eq!(parsed.total_rows, 250);
        assert_eq!(parsed.shown_rows, 200);
        assert_eq!(parsed.rows.len(), 200);

        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn 컬럼_150개면_100개만_보여주고_전체_개수는_유지한다() {
        let path = tmp_path("many_cols.parquet");
        let mut schema_src = String::from("message schema {\n");
        for i in 0..150 {
            schema_src.push_str(&format!("REQUIRED INT32 c{i};\n"));
        }
        schema_src.push('}');
        let schema = Arc::new(parse_message_type(&schema_src).unwrap());
        let file = File::create(&path).unwrap();
        let props = Arc::new(WriterProperties::builder().build());
        let mut writer = SerializedFileWriter::new(file, schema, props).unwrap();
        let mut rg = writer.next_row_group().unwrap();
        for _ in 0..150 {
            let mut col = rg.next_column().unwrap().unwrap();
            col.typed::<Int32Type>()
                .write_batch(&[1, 2], None, None)
                .unwrap();
            col.close().unwrap();
        }
        rg.close().unwrap();
        writer.close().unwrap();

        let json = parquet_preview(&path);
        let parsed: TablePreview = serde_json::from_str(&json).unwrap();
        assert!(parsed.error.is_none());
        assert_eq!(parsed.total_columns, 150);
        assert_eq!(parsed.shown_columns, 100);
        assert_eq!(parsed.columns.len(), 100);
        for row in &parsed.rows {
            assert_eq!(row.len(), 100);
        }

        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn max_file_bytes를_넘는_parquet도_table로_읽힌다() {
        let path = tmp_path("huge.parquet");
        let schema = Arc::new(
            parse_message_type("message schema { REQUIRED INT32 v; }").unwrap(),
        );
        let file = File::create(&path).unwrap();
        // 압축 없이 int32 반복 값을 대량으로 써서 파일 크기를 2MiB 이상으로 만든다.
        let props = Arc::new(
            WriterProperties::builder()
                .set_compression(parquet::basic::Compression::UNCOMPRESSED)
                .build(),
        );
        let mut writer = SerializedFileWriter::new(file, schema, props).unwrap();
        let mut rg = writer.next_row_group().unwrap();
        let mut col = rg.next_column().unwrap().unwrap();
        let values: Vec<i32> = (0..600_000).collect();
        col.typed::<Int32Type>()
            .write_batch(&values, None, None)
            .unwrap();
        col.close().unwrap();
        rg.close().unwrap();
        writer.close().unwrap();

        let size = std::fs::metadata(&path).unwrap().len();
        assert!(
            size > super::super::MAX_FILE_BYTES,
            "테스트 전제 실패: 파일이 {size} 바이트로 MAX_FILE_BYTES 이하입니다"
        );

        let root = path.parent().unwrap().to_path_buf();
        let fc = super::super::read_file(&root, "huge.parquet").unwrap();
        assert_eq!(fc.kind, super::super::FileKind::Table);
        let parsed: TablePreview = serde_json::from_str(&fc.content).unwrap();
        assert!(parsed.error.is_none());
        assert_eq!(parsed.total_rows, 600_000);
        assert_eq!(parsed.shown_rows, 200);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 깨진_파일은_table_kind에_error를_담아_반환한다() {
        let root = crate::testtmp::dir().join(format!(
            "praxis-fsapi-table-garbage-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("x.parquet");
        std::fs::write(&path, b"this is not a parquet file at all").unwrap();

        let fc = super::super::read_file(&root, "x.parquet").unwrap();
        assert_eq!(fc.kind, super::super::FileKind::Table);
        let parsed: TablePreview = serde_json::from_str(&fc.content).unwrap();
        assert!(parsed.error.is_some());
        assert_eq!(parsed.total_rows, 0);
        assert!(parsed.rows.is_empty());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 확장자_대소문자를_구분하지_않는다() {
        let root = crate::testtmp::dir().join(format!(
            "praxis-fsapi-table-case-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("X.PARQUET");
        write_small_file(&path);

        let fc = super::super::read_file(&root, "X.PARQUET").unwrap();
        assert_eq!(fc.kind, super::super::FileKind::Table);

        std::fs::remove_dir_all(&root).ok();
    }
}
