//! The rating inside an XMP packet: reading it and changing it.
//!
//! Cameras and other programs leave a run of blanks at the end of a packet so that it can
//! be changed without moving anything else in the file. A change that makes the text
//! longer or shorter is evened out there, keeping the packet's length.

use std::ops::Range;

/// A rating: 1 to 5 stars, 0 for none, or [`REJECTED`].
pub type Rating = i8;
/// Marks a picture as rejected, the way Adobe's programs store it.
pub const REJECTED: Rating = -1;
pub const MAX_RATING: Rating = 5;

const PROPERTY: &[u8] = b"xmp:Rating";
const NAMESPACE_ATTRIBUTE: &[u8] = b" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\"";
const NAMESPACE_DECLARED: &[u8] = b"xmlns:xmp=";
const DESCRIPTION: &[u8] = b"<rdf:Description";
const RDF_END: &[u8] = b"</rdf:RDF>";
const TRAILER: &[u8] = b"<?xpacket end";
/// Blanks put into a new packet, room for later changes.
const NEW_PACKET_PADDING: usize = 2048;

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|position| position + from)
}

/// Where the rating's value stands in `packet`, as an attribute or as an element.
fn value_span(packet: &[u8]) -> Option<Range<usize>> {
    let mut from = 0;
    while let Some(at) = find(packet, PROPERTY, from) {
        let after = at + PROPERTY.len();
        let closing = match (packet.get(after), packet.get(after + 1)) {
            (Some(b'='), Some(&quote @ (b'"' | b'\''))) => Some((after + 2, quote)),
            (Some(b'>'), _) => Some((after + 1, b'<')),
            _ => None,
        };
        if let Some((start, end_mark)) = closing {
            let length = packet.get(start..)?.iter().position(|&b| b == end_mark)?;
            return Some(start..start + length);
        }
        from = after;
    }
    None
}

/// The rating stated in `packet`. `None` when it states none.
pub fn read_rating(packet: &[u8]) -> Option<Rating> {
    let text = std::str::from_utf8(&packet[value_span(packet)?]).ok()?;
    let value: f32 = text.trim().parse().ok()?;
    Some(
        value
            .round()
            .clamp(f32::from(REJECTED), f32::from(MAX_RATING)) as Rating,
    )
}

/// `packet` with its rating set to `rating`. The length may differ from the original's.
/// `None` when the packet has no place to put a rating.
pub fn with_rating(packet: &[u8], rating: Rating) -> Option<Vec<u8>> {
    let value = rating.to_string();
    let mut changed = packet.to_vec();
    if let Some(span) = value_span(packet) {
        changed.splice(span, value.bytes());
        return Some(changed);
    }
    let mut attribute = format!(" xmp:Rating=\"{value}\"").into_bytes();
    if find(packet, NAMESPACE_DECLARED, 0).is_none() {
        attribute.extend_from_slice(NAMESPACE_ATTRIBUTE);
    }
    if let Some(description) = find(packet, DESCRIPTION, 0) {
        let at = description + DESCRIPTION.len();
        changed.splice(at..at, attribute);
        return Some(changed);
    }
    let at = find(packet, RDF_END, 0)?;
    let mut element = b"<rdf:Description rdf:about=\"\"".to_vec();
    element.extend_from_slice(&attribute);
    element.extend_from_slice(b"/>");
    changed.splice(at..at, element);
    Some(changed)
}

/// Brings `packet` to exactly `length` bytes by adding or removing blanks in front of its
/// trailer. `None` when there are not enough blanks to remove.
///
/// Blanks are taken from and given back to the same place, the end of the last line of
/// spaces, so that undoing a change gives the original bytes again.
pub fn fitted(mut packet: Vec<u8>, length: usize) -> Option<Vec<u8>> {
    if packet.len() == length {
        return Some(packet);
    }
    let trailer = find(&packet, TRAILER, 0)?;
    let line_breaks = packet[..trailer]
        .iter()
        .rev()
        .take_while(|byte| matches!(byte, b'\n' | b'\r'))
        .count();
    let line_end = trailer - line_breaks;
    if packet.len() < length {
        let missing = length - packet.len();
        packet.splice(line_end..line_end, std::iter::repeat_n(b' ', missing));
        return Some(packet);
    }
    let surplus = packet.len() - length;
    let spaces = packet[..line_end]
        .iter()
        .rev()
        .take_while(|byte| **byte == b' ')
        .count();
    if spaces >= surplus {
        packet.drain(line_end - surplus..line_end);
        return Some(packet);
    }
    let blanks = packet[..trailer]
        .iter()
        .rev()
        .take_while(|byte| byte.is_ascii_whitespace())
        .count();
    if blanks < surplus {
        return None;
    }
    packet.drain(trailer - surplus..trailer);
    Some(packet)
}

/// A complete packet that states only `rating`, with room for later changes.
pub fn new_packet(rating: Rating) -> Vec<u8> {
    let mut packet = format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
         <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
         <rdf:Description rdf:about=\"\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" xmp:Rating=\"{rating}\"/>\n\
         </rdf:RDF>\n\
         </x:xmpmeta>\n"
    )
    .into_bytes();
    for index in 0..NEW_PACKET_PADDING {
        packet.push(if index % 100 == 99 { b'\n' } else { b' ' });
    }
    packet.extend_from_slice(b"<?xpacket end=\"w\"?>");
    packet
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A packet as a Sony camera writes it into its RAW files, shortened.
    fn camera_packet(blanks: usize) -> Vec<u8> {
        let mut packet = b"<?xpacket begin='\xef\xbb\xbf' id='W5M0MpCehiHzreSzNTczkc9d'?>\n\
            <x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
            <rdf:Description rdf:about=\"\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\">\
            <xmp:Rating>0</xmp:Rating></rdf:Description></rdf:RDF></x:xmpmeta>"
            .to_vec();
        packet.extend(std::iter::repeat_n(b' ', blanks));
        packet.extend_from_slice(b"<?xpacket end='w'?>");
        packet
    }

    fn set(packet: &[u8], rating: Rating) -> Option<Vec<u8>> {
        fitted(with_rating(packet, rating)?, packet.len())
    }

    #[test]
    fn reads_ratings_written_as_element_or_attribute() {
        assert_eq!(read_rating(&camera_packet(10)), Some(0));
        assert_eq!(
            read_rating(b"<rdf:Description xmp:Rating=\"4\" xmp:Label=\"x\"/>"),
            Some(4)
        );
        assert_eq!(
            read_rating(b"<rdf:Description xmp:Rating='-1'/>"),
            Some(REJECTED)
        );
        assert_eq!(read_rating(b"<xmp:Rating>3.0</xmp:Rating>"), Some(3));
        assert_eq!(
            read_rating(b"<xmp:Rating>99</xmp:Rating>"),
            Some(MAX_RATING)
        );
        assert_eq!(
            read_rating(b"<rdf:Description xmp:CreatorTool=\"x\"/>"),
            None
        );
        assert_eq!(read_rating(b"<xmp:Rating>many</xmp:Rating>"), None);
    }

    #[test]
    fn a_star_rating_changes_one_byte() {
        let original = camera_packet(10);
        let rated = set(&original, 4).expect("fits");
        assert_eq!(rated.len(), original.len());
        assert_eq!(read_rating(&rated), Some(4));
        let differing = original.iter().zip(&rated).filter(|(a, b)| a != b).count();
        assert_eq!(differing, 1);
    }

    #[test]
    fn a_longer_value_uses_up_blanks_and_a_shorter_one_gives_them_back() {
        let original = camera_packet(10);
        let rejected = set(&original, REJECTED).expect("fits");
        assert_eq!(rejected.len(), original.len());
        assert_eq!(read_rating(&rejected), Some(REJECTED));
        assert!(rejected.ends_with(b"         <?xpacket end='w'?>"));
        assert!(!rejected.ends_with(b"          <?xpacket end='w'?>"));

        let restored = set(&rejected, 0).expect("fits");
        assert_eq!(restored, original);
    }

    #[test]
    fn blanks_come_from_the_line_before_the_trailer_and_go_back_there() {
        let mut original = camera_packet(0);
        let trailer = original.len() - b"<?xpacket end='w'?>".len();
        original.splice(trailer..trailer, b"\n      \n      \n".iter().copied());
        let rejected = set(&original, REJECTED).expect("fits");
        assert!(rejected.ends_with(b"\n      \n     \n<?xpacket end='w'?>"));
        assert_eq!(set(&rejected, 0).expect("fits"), original);
    }

    #[test]
    fn without_blanks_a_longer_value_does_not_fit() {
        let tight = camera_packet(0);
        assert!(set(&tight, 5).is_some());
        assert!(set(&tight, REJECTED).is_none());
        // Without a trailer there is no place to even out a change.
        assert!(set(b"<xmp:Rating>0</xmp:Rating>", REJECTED).is_none());
    }

    #[test]
    fn a_missing_rating_is_added_to_the_first_description() {
        let packet = b"<rdf:RDF><rdf:Description rdf:about=\"\" xmlns:dc=\"x\"/></rdf:RDF>    <?xpacket end=\"w\"?>";
        let rated = with_rating(packet, 2).expect("place found");
        assert_eq!(read_rating(&rated), Some(2));
        let text = String::from_utf8(rated).expect("utf-8");
        assert!(text.contains("<rdf:Description xmp:Rating=\"2\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" rdf:about=\"\""));

        let declared = b"<rdf:Description xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\"/>";
        let text =
            String::from_utf8(with_rating(declared, 1).expect("place found")).expect("utf-8");
        assert_eq!(text.matches("xmlns:xmp").count(), 1);

        let empty = b"<rdf:RDF></rdf:RDF>";
        assert_eq!(
            read_rating(&with_rating(empty, 5).expect("place found")),
            Some(5)
        );
        assert!(with_rating(b"no metadata here", 3).is_none());
    }

    #[test]
    fn a_new_packet_states_its_rating_and_leaves_room() {
        let packet = new_packet(3);
        assert_eq!(read_rating(&packet), Some(3));
        let rejected = set(&packet, REJECTED).expect("fits");
        assert_eq!(rejected.len(), packet.len());
        assert_eq!(read_rating(&rejected), Some(REJECTED));
        assert!(std::str::from_utf8(&packet).is_ok());
    }
}
