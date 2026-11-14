Class IadSm172.Ledger Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property Code As %String;

Property Amount As %Integer;

Property Memo As %String;

Property Tag1 As %String;

Property Tag2 As %String;

Storage SQLStorage
{
<SqlIdExpression>$i(^IadSm172Ledger)</SqlIdExpression>
<SQLMap name="Master">
<Data name="Amount">
<Delimiter>"|"</Delimiter>
<Piece>2</Piece>
</Data>
<Data name="Code">
<Delimiter>"|"</Delimiter>
<Piece>1</Piece>
</Data>
<Data name="Memo">
<Node>"memo"</Node>
</Data>
<Data name="Tag1">
<Delimiter>$C(1)</Delimiter>
<Node>"t"</Node>
<Piece>1</Piece>
</Data>
<Data name="Tag2">
<Delimiter>$C(1)</Delimiter>
<Node>"t"</Node>
<Piece>1+1</Piece>
</Data>
<Global>^IadSm172Ledger</Global>
<Subscript name="1">
<Expression>{ID}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172LedgerS</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
